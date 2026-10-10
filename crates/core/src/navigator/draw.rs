use super::*;

impl Navigator {
    pub fn frame(&mut self, renderer: &Renderer, scene: &mut Scene, f: &NavFrame) {
        self.panel_overlay = None;
        if !self.enabled && !self.city.open && !self.arrows && self.shown < 0.01 {
            return;
        }
        let target = if self.enabled { 1.0 } else { 0.0 };
        self.shown += (target - self.shown) * (1.0 - (-f.dt * 8.0).exp());
        if (self.shown - target).abs() < 0.005 {
            self.shown = target;
        }
        self.time += f.dt;
        if let Some(rx) = self.building.as_ref() {
            if let Ok((net, pos, streets, graph, surfaces)) = rx.try_recv() {
                self.surfaces = Some(std::sync::Arc::new(surfaces));
                self.city.roads = None;
                log::info!(
                    "navigator: the map's road network is there ({} lanes, {} streets named)",
                    net.lanes.len(),
                    streets.names.len()
                );
                self.streets = Some(std::sync::Arc::new(streets));
                self.graph = Some(std::sync::Arc::new(graph));
                self.global = Some(std::sync::Arc::new(net));
                self.stop_pos = std::sync::Arc::new(pos);
                self.global_version += 1;
                self.building = None;
                self.roads = None;
                self.route = Route {
                    version: self.route.version + 1,
                    ..Route::default()
                };
                self.route_mesh = RouteMesh::default();
                self.route_jam.clear();
            }
        }
        if ::legacy_config::env::var_os("OMSI_DEBUG_NAV").is_some() && self.time < 0.15 {
            log::info!(
                "navigator: stops {:?}",
                f.stops
                    .iter()
                    .map(|s| (
                        s.name.clone(),
                        s.object_id,
                        s.position != DVec3::ZERO,
                        self.stop_pos.contains_key(&s.object_id)
                    ))
                    .collect::<Vec<_>>()
            );
        }
        let f2;
        let f = if f.stops.iter().any(|s| s.position == DVec3::ZERO) {
            f2 = NavFrame {
                stops: f
                    .stops
                    .iter()
                    .filter_map(|s| {
                        if s.position == DVec3::ZERO {
                            self.stop_pos.get(&s.object_id).map(|p| NavStop {
                                position: *p,
                                ..s.clone()
                            })
                        } else {
                            Some(s.clone())
                        }
                    })
                    .collect(),
                ..f.clone_ref()
            };
            &f2
        } else {
            f
        };
        self.follow(f);
        self.bus_at = f.bus;
        self.stop_spots = f
            .stops
            .iter()
            .take(3)
            .map(|st| (st.position, st.name.clone(), f.heading, st.object_id))
            .collect();
        if ::legacy_config::env::var_os("OMSI_DEBUG_NAV").is_some() && (self.time % 1.0) < f.dt {
            log::info!(
                "navigator: route {} lanes (complete {}, provisional {}, at {}, on it {}, off for {:.1} s), {} stops ahead, next {:?}, key {:?}",
                self.route.lanes.len(),
                self.route.complete,
                self.route.provisional,
                self.route.progress,
                self.route.on_route,
                self.route.off_for,
                f.stops.len(),
                f.stops.first().map(|s| (
                    s.name.clone(),
                    s.position.x.round(),
                    s.position.y.round()
                )),
                self.route.key
            );
        }
        self.update_congestion(f);
        let want = (126.0 + f.speed_kmh as f64 * 2.5).clamp(126.0, 320.0);
        if self.first {
            self.zoom = want;
            self.cam_heading = f.heading;
        }
        self.zoom += (want - self.zoom) * ease(f.dt, 1.8);
        self.cam_heading += angle_diff(self.cam_heading, f.heading) * ease(f.dt, 0.3);
        let v = f.speed_kmh.abs() / 3.6;
        self.speed_avg += (v - self.speed_avg) * ease(f.dt, 20.0) as f32;
        self.dist_t -= f.dt;
        if self.dist_t <= 0.0 || self.first {
            self.dist_t = 0.4;
            let global = self.global.clone();
            let net = global.as_deref().or(f.traffic.map(|t| &t.net));
            self.next_dist = match (f.stops.first(), net) {
                (Some(s), Some(n)) if !self.route.lanes.is_empty() => self
                    .route_distance(n, s.position)
                    .or_else(|| Some((s.position - f.bus).truncate().length())),
                (Some(s), _) => Some((s.position - f.bus).truncate().length()),
                _ => None,
            };
            self.next_turn = net.and_then(|n| self.turn_ahead(n));
            self.street_here = if self.route.on_route {
                self.route
                    .lanes
                    .get(self.route.progress)
                    .and_then(|&l| self.street_of(l))
            } else {
                global
                    .as_deref()
                    .and_then(|g| g.nearest_lane_near(f.bus, LaneKind::Street))
                    .filter(|l| l.2 < 10.0)
                    .and_then(|l| self.street_of(l.0))
            }
                .map(str::to_string);
        }
        self.first = false;
        if !self.enabled && self.shown < 0.01 {
            self.panel_rect = [0.0; 4];
            if self.city.open {
                self.city(renderer, scene, f);
            }
            return;
        }

        let (sw, sh) = f.screen;
        let base = (sh * 0.33).max(300.0);
        let base = if f.follow_window {
            base
        } else {
            base.min(480.0)
        };
        let pw = (base * f.ui_scale).min((sh * 0.7).max(300.0)).round();
        let map_h = (pw * 0.62).round();
        let s = pw / 360.0;
        let has_bottom =
            self.show_stoplist && (!f.stops.is_empty() || !self.route.lanes.is_empty());
        let step = f.dt.clamp(0.0, 0.1) / 0.2;
        self.bottom_t = if has_bottom {
            (self.bottom_t + step).min(1.0)
        } else {
            (self.bottom_t - step).max(0.0)
        };
        let t = self.bottom_t;
        self.bottom_e = t * t * (3.0 - 2.0 * t);
        let want_turn = self.show_turn && self.next_turn.is_some();
        if want_turn {
            if let Some(t) = self.next_turn.clone() {
                self.turn_shown = Some(t);
            }
        }
        let tstep = f.dt.clamp(0.0, 0.1) / 0.25;
        self.turn_t = if want_turn {
            (self.turn_t + tstep).min(1.0)
        } else {
            (self.turn_t - tstep).max(0.0)
        };
        if self.turn_t <= 0.0 && !want_turn {
            self.turn_shown = None;
        }
        let tt = self.turn_t;
        self.turn_e = tt * tt * (3.0 - 2.0 * tt);
        let turn_animating =
            self.turn_t > 0.0 && self.turn_t < 1.0 || want_turn != (self.turn_t >= 1.0);
        if !f.stops.is_empty() {
            self.sched_rows = f.stops.len().clamp(1, 5) as f32;
        }
        let want_sched = self.schedule && has_bottom && !f.stops.is_empty();
        self.sched_t = if want_sched {
            (self.sched_t + step).min(1.0)
        } else {
            (self.sched_t - step).max(0.0)
        };
        let st = self.sched_t;
        self.sched_e = st * st * (3.0 - 2.0 * st);
        let top_px = if self.show_topbar { 34.0 } else { 0.0 };
        let bars = (top_px + 46.0 * self.bottom_e) * s;
        let sched = (self.sched_rows * 22.0 + 12.0) * s * self.sched_e;
        let ph = (map_h + bars + sched).round();
        let (w, h) = (pw as u32, ph as u32);
        let margin = (sh * 0.018).max(10.0).round();
        let touch = crate::platform::touch_controls();
        let right = self.corner.contains("right");
        let top = self.corner.contains("top") || touch;
        let x0 = if self.corner.contains("center") || touch {
            ((sw - pw) * 0.5).round()
        } else if right {
            sw - margin - pw
        } else {
            margin
        };
        let y0 = if top { margin } else { sh - margin - ph };

        if self.gpu.is_none() {
            self.gpu = Some(Gpu::new(
                &renderer.device,
                renderer.format(),
                map_samples(renderer.format()),
                self.atlas.size,
            ));
        }
        let resized = self.target.map(|t| (t.1, t.2) != (w, h)).unwrap_or(true);
        if resized {
            if let Some((t, _, _)) = self.target.take() {
                renderer.free_texture(scene, t);
                scene.premultiplied.remove(&t);
                scene.frosted.remove(&t);
            }
            let t = renderer.add_render_texture(scene, w, h);
            scene.premultiplied.insert(t);
            // the panel over a blur of what lies beneath it
            scene.frosted.insert(t, (CARD_RADIUS + CARD_GAP) * s);
            self.target = Some((t, w, h));
        }
        let (tex, _, _) = self.target.unwrap();
        let Some(view) = renderer.texture_view(scene, tex) else {
            return;
        };
        if !self.city.open
            && (resized || turn_animating || self.time - self.drawn_at >= NAV_REDRAW_S)
        {
            self.drawn_at = self.time;
            self.draw(renderer, &view, (w, h), map_h, f);
        }
        if !self.city.open {
            self.panel_overlay = Some(scene.overlays.len());
            scene.overlays.push((tex, [x0, y0, x0 + pw, y0 + ph]));
        }
        self.panel_rect = [x0, y0, x0 + pw, y0 + ph];
        if self.city.open {
            self.city(renderer, scene, f);
        }
    }

    pub(super) fn draw(
        &mut self,
        renderer: &Renderer,
        target: &wgpu::TextureView,
        size: (u32, u32),
        map_h: f32,
        f: &NavFrame,
    ) {
        let (pw, ph) = (size.0 as f32, size.1 as f32);
        let s = pw / 360.0;
        let wd = words();
        self.atlas.begin_frame();
        let panel = Rect::new(0.0, 0.0, pw, ph);
        // the corners nest: whatever sits `d` inside a rounded shape is rounded `d` less
        let gap = CARD_GAP * s;
        let card_r = CARD_RADIUS * s;
        let radius = card_r + gap;
        let top_h = if self.show_topbar {
            (34.0 * s).round()
        } else {
            0.0
        };
        let has_bottom = self.bottom_e > 0.001;
        // the map under the whole panel; the cards float above it
        let _ = (top_h, map_h, has_bottom);
        let map = panel;
        let vp = [map.x, map.y, map.w, map.h];

        let own = self.own_net.clone();
        let global = self.global.clone();
        let net = global
            .as_deref()
            .or(f.traffic.map(|t| &t.net))
            .or(own.as_deref());
        let lanes_now = net.map(|n| n.lanes.len()).unwrap_or(0);
        let surfaces = self.surfaces.clone();
        // Navigator 2.0 draws the ground the map lays; the lanes are the fallback while
        // that is being built
        let reach = if surfaces.is_some() {
            SURFACE_RADIUS * 0.4
        } else {
            ROAD_RADIUS * 0.45
        };
        let rebuild = match &self.roads {
            None => lanes_now > 0 || surfaces.is_some(),
            Some(r) => {
                (r.anchor - f.bus.truncate()).length() > reach
                    || (r.lanes_seen != lanes_now && self.time - r.built_at > 1.5)
            }
        };
        let mut road_verts = None;
        if rebuild {
            if net.is_some() || surfaces.is_some() {
                let anchor = f.bus.truncate();
                let mut p = Painter::new();
                let t0 = std::time::Instant::now();
                match (surfaces.as_deref(), self.graph.as_deref().filter(|_| global.is_some()), net) {
                    (Some(sm), _, _) => {
                        build_surfaces(&mut p, sm, anchor, anchor, SURFACE_RADIUS, false, 1.0)
                    }
                    (None, Some(g), _) => build_roads_from(&mut p, g, anchor),
                    (None, None, Some(net)) => build_roads(&mut p, net, anchor),
                    (None, None, None) => {}
                }
                if ::legacy_config::env::var_os("OMSI_DEBUG_NAV").is_some() {
                    log::info!(
                        "navigator: roads around ({:.0}, {:.0}) in {:.1} ms: {} vertices",
                        anchor.x,
                        anchor.y,
                        t0.elapsed().as_secs_f64() * 1000.0,
                        p.verts.len()
                    );
                }
                road_verts = Some(p.verts);
                self.roads = Some(Roads {
                    anchor,
                    lanes_seen: lanes_now,
                    verts: 0,
                    built_at: self.time,
                });
            }
        }

        let anchor = self
            .roads
            .as_ref()
            .map(|r| r.anchor)
            .unwrap_or(f.bus.truncate());
        let rel = |p: DVec3| Vec3::new((p.x - anchor.x) as f32, (p.y - anchor.y) as f32, 0.0);
        let hd = self.cam_heading.to_radians();
        let fwd = DVec2::new(hd.sin(), hd.cos());
        let look_at = f.bus.truncate() - anchor + fwd * self.zoom * 0.28;
        let pitch = PITCH.to_radians();
        let back = fwd * self.zoom * pitch.cos();
        let eye = DVec3::new(
            look_at.x - back.x,
            look_at.y - back.y,
            self.zoom * pitch.sin(),
        );
        let view = glam::camera::rh::view::look_at_mat4(
            eye.as_vec3(),
            Vec3::new(look_at.x as f32, look_at.y as f32, 0.0),
            Vec3::Z,
        );
        let clip_panel = [0.0, 0.0, pw, ph];
        let mut map_layer = Layer::world(
            view,
            FOV.to_radians(),
            vp,
            [map.x, map.y, map.right(), map.bottom()],
            radius,
            1.0,
        );
        // the bus just above the cards at the bottom: the picture is shifted up or down
        // (in clip space, so the perspective stays the camera's)
        let reserve = if self.show_topbar || self.bottom_e > 0.001 {
            (8.0 + 52.0 + 64.0) * s
                + if self.sched_e > 0.001 && !f.stops.is_empty() {
                    (f.stops.len().min(5) as f32 * 20.0 + 6.0) * s * self.sched_e
                } else {
                    0.0
                }
        } else {
            30.0 * s
        };
        if let Some(bp) = project(map_layer.view_proj, vp, rel(f.bus)) {
            let want = (ph - reserve).max(ph * 0.45);
            let shift = 2.0 * (bp.y - want) / ph.max(1.0);
            map_layer.view_proj = Mat4::from_translation(Vec3::new(0.0, shift, 0.0)) * map_layer.view_proj;
        }
        let vpm = map_layer.view_proj;

        let mut route_verts = None;
        let mut route_cut = Layer::WHOLE_ROUTE;
        let route_net = global.as_deref().or(f.traffic.map(|t| &t.net));
        if let (Some(rn), true) = (route_net, !self.route.lanes.is_empty()) {
            let print = lanes_print(&self.route.lanes);
            if self.route_cum.0 != print {
                let mut cum = Vec::with_capacity(self.route.lanes.len() + 1);
                let mut acc = 0.0f64;
                cum.push(0.0);
                for &l in &self.route.lanes {
                    acc += rn.lanes.get(l).map(|l| l.length() as f64).unwrap_or(0.0);
                    cum.push(acc);
                }
                self.route_cum = (print, cum);
            }
            let cum = &self.route_cum.1;
            let k = self.route.progress.min(self.route.lanes.len() - 1);
            let bus_s = cum[k] + self.route.s as f64;
            let total = cum[cum.len() - 1];
            let px = ROUTE_PX * s;
            let m = &self.route_mesh;
            if m.lanes != print
                || m.jam != self.jam_version
                || m.anchor != anchor
                || m.px != px
                || k < m.from
                || (bus_s + ROUTE_AHEAD + 100.0 > m.end && m.end < total)
            {
                let mut p = Painter::new();
                let reach = bus_s - cum[k] + ROUTE_AHEAD + 1500.0;
                build_route_line(
                    &mut p,
                    rn,
                    &self.route.lanes[k..],
                    cum[k],
                    anchor,
                    &self.route_jam,
                    reach,
                    px,
                );
                self.route_mesh = RouteMesh {
                    lanes: print,
                    jam: self.jam_version,
                    anchor,
                    px,
                    from: k,
                    end: (cum[k] + reach).min(total),
                    verts: p.len(),
                };
                if ::legacy_config::env::var_os("OMSI_DEBUG_NAV").is_some() {
                    log::info!(
                        "navigator: route line built from route lane {k} ({:.0} m to {:.0} m along): {} vertices",
                        cum[k],
                        self.route_mesh.end,
                        self.route_mesh.verts
                    );
                }
                route_verts = Some(p.verts);
            }
            // the route past the next turn is dimmed; where that begins glides after the
            // turn ahead rather than jumping each time it is worked out again
            let want = self
                .next_turn
                .as_ref()
                .filter(|_| self.route.on_route)
                .map(|t| t.2 + 30.0)
                .unwrap_or(ROUTE_AHEAD + 200.0);
            let dt = (self.time - self.dim_at).clamp(0.0, 0.2);
            self.dim_at = self.time;
            self.dim_ahead = if self.dim_ahead <= 0.0 {
                want
            } else {
                self.dim_ahead + (want - self.dim_ahead) * ease(dt, 0.35)
            };
            let dim = bus_s + self.dim_ahead;
            route_cut = [bus_s as f32, dim as f32, (bus_s + ROUTE_AHEAD) as f32];
        } else if self.route_mesh.verts != 0 {
            route_verts = Some(Vec::new());
            self.route_mesh = RouteMesh::default();
        }
        map_layer.route = route_cut;

        // the interface opacity setting: how much of the picture shows through the map
        let o = if self.cockpit_display {
            1.0
        } else {
            self.opacity.clamp(0.2, 1.0)
        };
        let mut bg = Painter::new();
        bg.rounded(panel, radius, Color::rgba(16, 16, 19, o));
        let n_bg = bg.len();

        let mut dy = Painter::new();
        if let Some(net) = f.traffic.map(|t| &t.net) {
            for (&lane, &c) in &self.congestion {
                let lv = level(c);
                if lv < 3 {
                    continue;
                }
                let Some(l) = net.lanes.get(lane) else {
                    continue;
                };
                if (l.start() - f.bus).truncate().length() > 900.0 {
                    continue;
                }
                let pts: Vec<Vec3> = l.points.iter().map(|p| rel(*p)).collect();
                dy.ribbon(
                    &pts,
                    l.width.max(2.5) * 0.6,
                    2.0,
                    LEVEL[lv].alpha(0.8),
                    true,
                );
            }
        }
        let n_traffic = dy.len();
        if let Some(t) = f.traffic.filter(|_| self.show_ai) {
            let reach = self.zoom * 3.5 + 150.0;
            marks::vehicles(&mut dy, t, &rel, |q| (q - f.bus).truncate().length() <= reach, true);
        }
        let n_world = dy.len();

        // what stands on the map, sharp above it: stops and the bus
        let mut pins = Painter::new();
        let n_stops = f.stops.len();
        let markers = spaced_markers(
            f.stops.iter().enumerate().filter_map(|(k, st)| {
                project(vpm, vp, rel(st.position))
                    .filter(|p| map.contains(*p))
                    .map(|p| (k, p))
            }),
            20.0 * s,
        );
        for (k, sp) in markers.into_iter().rev() {
            marks::stop_pin(&mut pins, &mut self.atlas, sp, marks::Pin::of(k, n_stops), s * 1.05);
        }
        if let Some(bp) = project(vpm, vp, rel(f.bus)) {
            let a = (angle_diff(self.cam_heading, f.heading) as f32).to_radians();
            marks::own_arrow(&mut pins, bp, a, 9.0 * s, ACCENT, None);
        }

        // the cards: each a frosted pane (`panes`) with what it says on top (`ui`)
        let mut panes: Vec<(Rect, f32, f32)> = Vec::new();
        let mut ui = Painter::new();
        let miles = uses_miles(f.units);
        let bottom_e = self.bottom_e;
        let base_h = 52.0 * s;
        let sched_h = if self.sched_e > 0.001 && !f.stops.is_empty() {
            (f.stops.len().min(5) as f32 * 20.0 + 6.0) * s * self.sched_e
        } else {
            0.0
        };
        let lift = (1.0 - bottom_e) * 16.0 * s;
        let row_bottom = ph - gap;

        // speed, bottom left, with the limit on its corner
        let mut stop_x = gap;
        if self.show_topbar {
            let card = Rect::new(gap, row_bottom - base_h, base_h, base_h);
            let limit = net.and_then(|n| {
                let lane = if self.route.on_route {
                    self.route.lanes.get(self.route.progress).copied()
                } else {
                    None
                };
                let lane = lane.or_else(|| {
                    n.nearest_lane_near(f.bus, LaneKind::Street)
                        .filter(|l| l.2 < 8.0)
                        .map(|l| l.0)
                })?;
                let v = n.lanes.get(lane)?.speed_limit_kmh;
                (v > 1.0 && v < 200.0).then_some(v)
            });
            let over = limit
                .map(|v| ((f.speed_kmh.abs() - v - 1.0) / 4.0).clamp(0.0, 1.0))
                .unwrap_or(0.0);
            let over = over * over * (3.0 - 2.0 * over);
            panes.push((card, card_r, 1.0));
            if over > 0.0 {
                ui.rounded(card, card_r, Color::rgba(240, 64, 56, 0.22 * over));
            }
            let speed_color = TEXT.mix(Color::rgba(255, 92, 84, 1.0), over);
            let num = format!("{:.0}", speed(f.speed_kmh.abs(), miles));
            ui.text(
                &mut self.atlas,
                &self.fonts,
                &num,
                23.0 * s,
                Weight::Bold,
                Vec2::new(card.center().x, card.y + 29.0 * s),
                Align::Center,
                speed_color,
            );
            ui.text(
                &mut self.atlas,
                &self.fonts,
                if miles { "mph" } else { wd.kmh.as_str() },
                10.0 * s,
                Weight::Medium,
                Vec2::new(card.center().x, card.y + 43.0 * s),
                Align::Center,
                TEXT_DIM,
            );
            if let Some(v) = limit {
                let c = Vec2::new(card.right() - 3.0 * s, card.y + 3.0 * s);
                ui.circle(c + Vec2::new(0.0, 1.0 * s), 12.0 * s, Color::rgba(0, 0, 0, 0.35));
                ui.circle(c, 11.0 * s, Color::rgba(214, 38, 38, 1.0));
                ui.circle(c, 8.4 * s, Color::rgba(246, 246, 246, 1.0));
                let t = format!("{:.0}", speed((v / 5.0).round() * 5.0, miles));
                let px = if t.len() > 2 { 8.5 } else { 10.5 } * s;
                ui.text(
                    &mut self.atlas,
                    &self.fonts,
                    &t,
                    px,
                    Weight::Black,
                    Vec2::new(c.x, c.y + self.fonts.cap_height(px, Weight::Black) * 0.5),
                    Align::Center,
                    Color::rgba(15, 15, 15, 1.0),
                );
            }
            stop_x = card.right() + 6.0 * s;

            // the clock, top right
            let hh = (f.time / 3600.0) as i32 % 24;
            let mm = ((f.time % 3600.0) / 60.0) as i32;
            let time_text = format!("{hh:02}:{mm:02}");
            let day_text = wd.days[f.weekday.clamp(0, 6) as usize].as_str();
            let tw = self.fonts.width(&time_text, 13.0 * s, Weight::Bold);
            let dw = self.fonts.width(day_text, 10.5 * s, Weight::Medium);
            let chip = Rect::new(pw - gap - (tw + dw + 25.0 * s), gap, tw + dw + 25.0 * s, 26.0 * s);
            panes.push((chip, 13.0 * s, 1.0));
            let base = chip.center().y + self.fonts.cap_height(13.0 * s, Weight::Bold) * 0.5;
            ui.text(
                &mut self.atlas,
                &self.fonts,
                day_text,
                10.5 * s,
                Weight::Medium,
                Vec2::new(chip.x + 10.0 * s, base),
                Align::Left,
                TEXT_DIM,
            );
            ui.text(
                &mut self.atlas,
                &self.fonts,
                &time_text,
                13.0 * s,
                Weight::Bold,
                Vec2::new(chip.right() - 10.0 * s, base),
                Align::Right,
                TEXT,
            );
        }

        // the next manoeuvre, top left
        if let Some((dir, angle, dist, street)) = self.turn_shown.clone().as_ref() {
            let ta = self.turn_e;
            let slide = (1.0 - ta) * -12.0 * s;
            let icon = match *dir {
                2 => "u_turn_left",
                -1 if *angle < 60.0 => "turn_slight_left",
                1 if *angle < 60.0 => "turn_slight_right",
                -1 => "turn_left",
                _ => "turn_right",
            };
            let t = rounded_distance(*dist, miles, 10.0);
            let room = pw * 0.66 - 56.0 * s;
            let street = street
                .as_deref()
                .map(|n| self.fonts.fit(n, 11.5 * s, Weight::Medium, room));
            let tw = self.fonts.width(&t, 18.0 * s, Weight::Bold);
            let sw_ = street
                .as_deref()
                .map(|n| self.fonts.width(n, 11.5 * s, Weight::Medium))
                .unwrap_or(0.0);
            let card = Rect::new(
                gap,
                gap + slide,
                56.0 * s + tw.max(sw_),
                if street.is_some() { 48.0 } else { 42.0 } * s,
            );
            panes.push((card, card_r, ta));
            let inset = 6.0 * s;
            let tile = Rect::new(card.x + inset, card.center().y - 17.0 * s, 34.0 * s, 34.0 * s);
            ui.rounded(tile, card_r - inset, ACCENT.alpha(ta));
            ui.icon(
                &mut self.atlas,
                icon,
                tile.center(),
                24.0 * s,
                Color::rgba(20, 14, 6, ta),
            );
            let x = tile.right() + 10.0 * s;
            let (y1, y2) = if street.is_some() {
                (card.y + 23.0 * s, card.y + 39.0 * s)
            } else {
                (card.center().y + self.fonts.cap_height(18.0 * s, Weight::Bold) * 0.5, 0.0)
            };
            ui.text(
                &mut self.atlas,
                &self.fonts,
                &t,
                18.0 * s,
                Weight::Bold,
                Vec2::new(x, y1),
                Align::Left,
                TEXT.alpha(ta),
            );
            if let Some(n) = street.as_deref() {
                ui.text(
                    &mut self.atlas,
                    &self.fonts,
                    n,
                    11.5 * s,
                    Weight::Medium,
                    Vec2::new(x, y2),
                    Align::Left,
                    TEXT_DIM.alpha(ta),
                );
            }
        }

        // the next stop, bottom right: line, name, how far and when, the delay, and with the
        // timetable the stops after it
        let note = if self.route.note > 0.0 {
            Some((wd.recalculated.clone(), ON_TIME))
        } else if self.route.joined
            && !self.route.lanes.is_empty()
            && !self.route.on_route
            && self.route.off_for > OFF_ROUTE_AFTER
        {
            Some((
                if self.route.off_for < OFF_ROUTE_AFTER + 20.0 {
                    wd.rerouting.clone()
                } else {
                    wd.off_route.clone()
                },
                WARN,
            ))
        } else {
            None
        };
        let jam_note = (self.jam_cost >= 30.0).then(|| {
            let what = if self.jam_cost >= 60.0 {
                wd.jam.as_str()
            } else {
                wd.slow.as_str()
            };
            (
                format!("{what} +{:.0} min", (self.jam_cost / 60.0).max(1.0).round()),
                if self.jam_cost >= 60.0 { LATE } else { WARN },
            )
        });
        let mut cards_top = if self.show_topbar {
            row_bottom - base_h
        } else {
            ph
        };
        if bottom_e > 0.001 {
            let h = base_h + sched_h;
            let card = Rect::new(stop_x, row_bottom - h + lift, pw - gap - stop_x, h);
            cards_top = cards_top.min(card.y);
            let a = bottom_e;
            panes.push((card, card_r, a));
            let inner = card.pad(11.0 * s, 0.0);
            let base1 = card.y + 21.0 * s;
            let base2 = card.y + 38.0 * s;
            match f.stops.first() {
                Some(st) => {
                    let mut x = inner.x;
                    if let Some(line) = f.line.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
                        let lw = self.fonts.width(line, 11.5 * s, Weight::Bold) + 11.0 * s;
                        let badge = Rect::new(x, base1 - 12.5 * s, lw, 17.0 * s);
                        ui.rounded(badge, 5.0 * s, ACCENT.alpha(a));
                        ui.text_in(
                            &mut self.atlas,
                            &self.fonts,
                            line,
                            11.5 * s,
                            Weight::Bold,
                            badge,
                            Align::Center,
                            Color::rgba(18, 14, 8, a),
                        );
                        x += lw + 7.0 * s;
                    }
                    let bell = if f.stop_requested { 20.0 * s } else { 0.0 };
                    if f.stop_requested {
                        ui.icon(
                            &mut self.atlas,
                            "stop_request",
                            Vec2::new(inner.right() - 8.0 * s, base1 - 4.5 * s),
                            18.0 * s,
                            STOP_REQUEST.alpha(a),
                        );
                    }
                    let name = if n_stops == 1 {
                        format!("{} · {}", st.name.trim(), wd.last_stop)
                    } else {
                        st.name.trim().to_string()
                    };
                    let name = self
                        .fonts
                        .fit(&name, 13.5 * s, Weight::Bold, inner.right() - x - bell);
                    ui.text(
                        &mut self.atlas,
                        &self.fonts,
                        &name,
                        13.5 * s,
                        Weight::Bold,
                        Vec2::new(x, base1),
                        Align::Left,
                        TEXT.alpha(a),
                    );
                    let mut pill_w = 0.0;
                    if let Some(d) = f.delay {
                        let (txt, c) = if d > 59.0 {
                            (
                                format!("+{}:{:02}", (d / 60.0) as i32, (d % 60.0) as i32),
                                LATE,
                            )
                        } else if d < -59.0 {
                            (
                                format!("−{}:{:02}", (-d / 60.0) as i32, (-d % 60.0) as i32),
                                EARLY,
                            )
                        } else {
                            (wd.on_time.to_string(), ON_TIME)
                        };
                        let px = 11.0 * s;
                        pill_w = self.fonts.width(&txt, px, Weight::Bold) + 14.0 * s;
                        let pill = Rect::new(inner.right() - pill_w, base2 - 12.0 * s, pill_w, 17.0 * s);
                        ui.rounded(pill, 8.5 * s, c.alpha(0.18 * a));
                        ui.text_in(
                            &mut self.atlas,
                            &self.fonts,
                            &txt,
                            px,
                            Weight::Bold,
                            pill,
                            Align::Center,
                            c.alpha(a),
                        );
                    }
                    let mut parts = Vec::new();
                    if let Some(d) = self.next_dist {
                        parts.push(rounded_distance(d, miles, 0.0));
                        let secs = d / (self.speed_avg.max(5.0) as f64);
                        parts.push(if secs < 60.0 {
                            "<1 min".to_string()
                        } else {
                            format!("{:.0} min", (secs / 60.0).round())
                        });
                    }
                    parts.push(clock(st.arrival));
                    let (line2, c2) = match (&note, &jam_note) {
                        (Some((t, c)), _) => (t.clone(), *c),
                        (None, Some((t, c))) => (format!("{}  ·  {t}", parts.join("  ·  ")), *c),
                        (None, None) => (parts.join("  ·  "), TEXT_DIM),
                    };
                    let line2 = self.fonts.fit(
                        &line2,
                        11.5 * s,
                        Weight::Medium,
                        inner.w - pill_w - 6.0 * s,
                    );
                    ui.text(
                        &mut self.atlas,
                        &self.fonts,
                        &line2,
                        11.5 * s,
                        Weight::Medium,
                        Vec2::new(inner.x, base2),
                        Align::Left,
                        c2.alpha(a),
                    );
                    if sched_h > 0.0 {
                        let late = f.delay.unwrap_or(0.0);
                        let ea = a * self.sched_e;
                        let mut y = card.y + base_h;
                        ui.rect(
                            Rect::new(inner.x, y - 1.0 * s, inner.w, 1.0),
                            Color::WHITE.alpha(0.07 * ea),
                        );
                        for st in f.stops.iter().take(5) {
                            let b = y + 14.0 * s;
                            ui.text(
                                &mut self.atlas,
                                &self.fonts,
                                &clock(st.arrival),
                                11.0 * s,
                                Weight::Bold,
                                Vec2::new(inner.x, b),
                                Align::Left,
                                TEXT_DIM.alpha(ea),
                            );
                            let n = self.fonts.fit(
                                st.name.trim(),
                                12.0 * s,
                                Weight::Medium,
                                inner.w - 90.0 * s,
                            );
                            ui.text(
                                &mut self.atlas,
                                &self.fonts,
                                &n,
                                12.0 * s,
                                Weight::Medium,
                                Vec2::new(inner.x + 42.0 * s, b),
                                Align::Left,
                                TEXT.alpha(ea),
                            );
                            ui.text(
                                &mut self.atlas,
                                &self.fonts,
                                &clock((st.arrival + late).rem_euclid(86400.0)),
                                11.0 * s,
                                Weight::Medium,
                                Vec2::new(inner.right(), b),
                                Align::Right,
                                if late > 59.0 {
                                    LATE
                                } else if late < -59.0 {
                                    EARLY
                                } else {
                                    TEXT_DIM
                                }
                                .alpha(ea),
                            );
                            y += 20.0 * s;
                        }
                    }
                }
                None => {
                    if let Some(t) = f.terminus.clone().filter(|t| !t.trim().is_empty()) {
                        let t = self.fonts.fit(t.trim(), 13.0 * s, Weight::Medium, inner.w);
                        ui.text(
                            &mut self.atlas,
                            &self.fonts,
                            &t,
                            13.0 * s,
                            Weight::Medium,
                            Vec2::new(
                                inner.x,
                                card.center().y + self.fonts.cap_height(13.0 * s, Weight::Medium) * 0.5,
                            ),
                            Align::Left,
                            TEXT_DIM.alpha(a),
                        );
                    }
                }
            }
        }

        // the street the bus is in, a small pane above the cards
        if let Some(n) = self.street_here.as_deref() {
            let px = 11.0 * s;
            let n = self.fonts.fit(n, px, Weight::Medium, pw * 0.6);
            let w = self.fonts.width(&n, px, Weight::Medium) + 20.0 * s;
            let r = Rect::new(pw * 0.5 - w * 0.5, cards_top - 6.0 * s - 20.0 * s, w, 20.0 * s);
            panes.push((r, 10.0 * s, 1.0));
            ui.text_in(&mut self.atlas, &self.fonts, &n, px, Weight::Medium, r, Align::Center, TEXT);
        }

        // the panes: a soft shadow, the blurred map, a dark tint and a fine light rim
        let mut shadows = Painter::new();
        let mut frost = Painter::new();
        let mut tint = Painter::new();
        for &(r, rad, a) in &panes {
            if a <= 0.01 {
                continue;
            }
            shadows.shadow(
                Rect::new(r.x, r.y + 2.0 * s, r.w, r.h),
                rad,
                12.0 * s,
                Color::rgba(0, 0, 0, 0.32 * a),
            );
            frost.image_rounded(r, rad, panel, Color::WHITE.alpha(a), true);
            tint.rounded(r, rad, Color::rgba(18, 18, 22, 0.58 * a));
            tint.rounded_gradient(
                r,
                rad,
                Color::WHITE.alpha(0.06 * a),
                Color::WHITE.alpha(0.0),
            );
            tint.rounded_border(r, rad, 1.0_f32.max(s), Color::WHITE.alpha(0.1 * a));
        }
        tint.rounded_border(panel, radius, 1.0_f32.max(s), Color::WHITE.alpha(0.08));

        let (Some(gpu), device, queue) = (self.gpu.as_mut(), &renderer.device, &renderer.queue)
        else {
            return;
        };
        glass::Glass::ensure(&mut self.glass, renderer, gpu, (size.0, size.1));
        let Some(glass) = self.glass.as_mut() else {
            return;
        };
        if let Some(v) = road_verts {
            if let Some(r) = self.roads.as_mut() {
                r.verts = v.len();
            }
            gpu.upload(device, queue, 0, &v);
        }
        if let Some(v) = route_verts {
            gpu.upload(device, queue, 1, &v);
        }
        gpu.upload_atlas(queue, &mut self.atlas);

        // 1: the map into its texture
        let mut world = bg.verts;
        world.extend(dy.verts);
        gpu.upload(device, queue, 2, &world);
        let flat = Layer::flat(clip_panel, radius, 1.0);
        let mut roads_layer = map_layer;
        // roads let the picture through a little with the ground; the route never
        roads_layer.opacity = 0.45 + 0.55 * o;
        let roads_n = self.roads.as_ref().map(|r| r.verts as u32).unwrap_or(0);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("navigator map"),
        });
        gpu.render(
            device,
            queue,
            &mut enc,
            glass.map_view(),
            size,
            Some(wgpu::Color::TRANSPARENT),
            &[flat, roads_layer, map_layer],
            &[
                Draw {
                    buffer: 2,
                    range: 0..n_bg,
                    layer: 0,
                    texture: 0,
                },
                Draw {
                    buffer: 0,
                    range: 0..roads_n,
                    layer: 1,
                    texture: 0,
                },
                Draw {
                    buffer: 2,
                    range: n_bg..n_bg + n_traffic,
                    layer: 2,
                    texture: 0,
                },
                // the traffic under the route: parked cars must not cut the line up
                Draw {
                    buffer: 2,
                    range: n_bg + n_traffic..n_bg + n_world,
                    layer: 2,
                    texture: 0,
                },
                Draw {
                    buffer: 1,
                    range: 0..self.route_mesh.verts,
                    layer: 2,
                    texture: 0,
                },
            ],
        );
        queue.submit([enc.finish()]);

        // 2: the map blurred for the panes
        if !panes.is_empty() {
            glass.blur(renderer);
        }

        // 3: the panel: the map, its pins, the panes and what they say
        let mut map_img = Painter::new();
        map_img.image_rounded(panel, radius, panel, Color::WHITE, true);
        let mut all = map_img.verts;
        let n_map = all.len() as u32;
        all.extend(pins.verts);
        all.extend(shadows.verts);
        let n_flat = all.len() as u32;
        all.extend(frost.verts);
        let n_frost = all.len() as u32;
        all.extend(tint.verts);
        all.extend(ui.verts);
        gpu.upload(device, queue, 6, &all);
        let mut panel_layer = Layer::flat(clip_panel, radius, 1.0);
        panel_layer.opacity *= self.shown;
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("navigator"),
        });
        gpu.render(
            device,
            queue,
            &mut enc,
            target,
            size,
            Some(wgpu::Color::TRANSPARENT),
            &[panel_layer],
            &[
                Draw {
                    buffer: 6,
                    range: 0..n_map,
                    layer: 0,
                    texture: glass.map_id,
                },
                Draw {
                    buffer: 6,
                    range: n_map..n_flat,
                    layer: 0,
                    texture: 0,
                },
                Draw {
                    buffer: 6,
                    range: n_flat..n_frost,
                    layer: 0,
                    texture: glass.blur_id,
                },
                Draw {
                    buffer: 6,
                    range: n_frost..all.len() as u32,
                    layer: 0,
                    texture: 0,
                },
            ],
        );
        queue.submit([enc.finish()]);
    }
}
