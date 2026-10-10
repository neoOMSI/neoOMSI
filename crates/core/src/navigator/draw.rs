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
            }
            let t = renderer.add_render_texture(scene, w, h);
            scene.premultiplied.insert(t);
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
        let radius = 8.0 * s;
        let top_h = if self.show_topbar {
            (34.0 * s).round()
        } else {
            0.0
        };
        let has_bottom = self.bottom_e > 0.001;
        let map = Rect::new(0.0, top_h, pw, map_h);
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
            0.0,
            1.0,
        );
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

        let mut bg = Painter::new();
        bg.rounded(
            panel,
            radius,
            if self.cockpit_display {
                Color::rgba(18, 18, 20, 1.0)
            } else {
                PANEL
            },
        );
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

        let mut ui = Painter::new();
        if let Some((dir, angle, dist, street)) = self.turn_shown.clone().as_ref() {
            let ta = self.turn_e;
            let slide = (1.0 - ta) * -14.0 * s;
            let icon = match *dir {
                2 => "u_turn_left",
                -1 if *angle < 60.0 => "turn_slight_left",
                1 if *angle < 60.0 => "turn_slight_right",
                -1 => "turn_left",
                _ => "turn_right",
            };
            let t = rounded_distance(*dist, uses_miles(f.units), 10.0);
            let tw = self.fonts.width(&t, 14.0 * s, Weight::Bold);
            let street = street.as_deref().map(|n| {
                self.fonts
                    .fit(n, 12.0 * s, Weight::Medium, map.w * 0.62 - 50.0 * s - tw)
            });
            let sw_ = street
                .as_deref()
                .map(|n| self.fonts.width(n, 12.0 * s, Weight::Medium) + 10.0 * s)
                .unwrap_or(0.0);
            let b = Rect::new(
                map.x + 8.0 * s,
                map.y + 8.0 * s + slide,
                44.0 * s + tw + sw_,
                34.0 * s,
            );
            ui.rounded(b, 8.0 * s, CARD.alpha(ta));
            ui.rounded_border(b, 8.0 * s, 1.0_f32.max(s), HAIR.alpha(ta));
            ui.icon(
                &mut self.atlas,
                icon,
                Vec2::new(b.x + 18.0 * s, b.center().y),
                24.0 * s,
                ACCENT.alpha(ta),
            );
            ui.text_in(
                &mut self.atlas,
                &self.fonts,
                &t,
                14.0 * s,
                Weight::Bold,
                Rect::new(b.x + 34.0 * s, b.y, tw + 4.0, b.h),
                Align::Left,
                TEXT.alpha(ta),
            );
            if let Some(n) = street.as_deref() {
                ui.text_in(
                    &mut self.atlas,
                    &self.fonts,
                    n,
                    12.0 * s,
                    Weight::Medium,
                    Rect::new(b.x + 42.0 * s + tw, b.y, sw_, b.h),
                    Align::Left,
                    TEXT_DIM.alpha(ta),
                );
            }
        }
        if let Some(n) = self.street_here.as_deref() {
            let px = 11.5 * s;
            let n = self.fonts.fit(n, px, Weight::Medium, map.w * 0.7);
            let w = self.fonts.width(&n, px, Weight::Medium) + 14.0 * s;
            let r = Rect::new(
                map.center().x - w * 0.5,
                map.bottom() - 24.0 * s,
                w,
                18.0 * s,
            );
            ui.rounded(r, 9.0 * s, CARD);
            ui.rounded_border(r, 9.0 * s, 1.0_f32.max(s), HAIR);
            ui.text_in(
                &mut self.atlas,
                &self.fonts,
                &n,
                px,
                Weight::Medium,
                r,
                Align::Center,
                STREET,
            );
        }
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
            marks::stop_pin(&mut ui, &mut self.atlas, sp, marks::Pin::of(k, n_stops), s * 1.05);
        }
        if let Some(bp) = project(vpm, vp, rel(f.bus)) {
            let a = (angle_diff(self.cam_heading, f.heading) as f32).to_radians();
            marks::own_arrow(&mut ui, bp, a, 9.0 * s, ACCENT, Some(ACCENT));
        }

        let pad = 11.0 * s;
        let miles = uses_miles(f.units);
        if self.show_topbar {
            let top = Rect::new(0.0, 0.0, pw, top_h);
            ui.rect(top, BAR);
            ui.rect(
                Rect::new(0.0, top.bottom() - 1.0_f32.max(s), pw, 1.0_f32.max(s)),
                HAIR,
            );
            let base = top.y + top.h * 0.5 + self.fonts.cap_height(22.0 * s, Weight::Bold) * 0.5;
            let mut x = pad;
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
            let speed_color = TEXT.mix(
                Color::rgba(240, 64, 56, 1.0),
                over * over * (3.0 - 2.0 * over),
            );
            x += ui.text(
                &mut self.atlas,
                &self.fonts,
                &format!("{:.0}", speed(f.speed_kmh.abs(), miles)),
                22.0 * s,
                Weight::Bold,
                Vec2::new(x, base),
                Align::Left,
                speed_color,
            );
            x += 4.0 * s;
            x += ui.text(
                &mut self.atlas,
                &self.fonts,
                if miles { "mph" } else { wd.kmh.as_str() },
                14.0 * s,
                Weight::Medium,
                Vec2::new(x, base),
                Align::Left,
                TEXT_DIM,
            );
            let stop_size = 26.0 * s;
            x += 8.0 * s;
            if f.stop_requested {
                ui.icon(
                    &mut self.atlas,
                    "stop_request",
                    Vec2::new(x + stop_size * 0.5, top.center().y),
                    stop_size,
                    STOP_REQUEST,
                );
            }
            x += stop_size;
            if let Some(v) = limit {
                x += 10.0 * s;
                let c = Vec2::new(x + 10.0 * s, top.center().y);
                ui.circle(c, 10.5 * s, Color::rgba(200, 40, 40, 1.0));
                ui.circle(c, 8.3 * s, Color::rgba(235, 235, 235, 1.0));
                let t = format!("{:.0}", speed((v / 5.0).round() * 5.0, miles));
                let px = if t.len() > 2 { 9.5 } else { 11.5 } * s;
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
            let hh = (f.time / 3600.0) as i32 % 24;
            let mm = ((f.time % 3600.0) / 60.0) as i32;
            let time_text = format!("{hh:02}:{mm:02}");
            let day_text = wd.days[f.weekday.clamp(0, 6) as usize].as_str();
            let time_w = self.fonts.width(&time_text, 14.0 * s, Weight::Bold);
            ui.text(
                &mut self.atlas,
                &self.fonts,
                &time_text,
                14.0 * s,
                Weight::Bold,
                Vec2::new(pw - pad, base),
                Align::Right,
                TEXT,
            );
            ui.text(
                &mut self.atlas,
                &self.fonts,
                day_text,
                12.0 * s,
                Weight::Medium,
                Vec2::new(pw - pad - time_w - 5.0 * s, base),
                Align::Right,
                TEXT_DIM,
            );
        }

        let bottom = Rect::new(0.0, map.bottom(), pw, 46.0 * s * self.bottom_e);
        if has_bottom {
            ui.rect(bottom, BAR);
            ui.rect(Rect::new(0.0, bottom.y, pw, 1.0_f32.max(s)), HAIR);
        }
        let stop_row = if f.stops.is_empty() {
            Rect::new(pad, bottom.y, pw - 2.0 * pad, bottom.h)
        } else {
            Rect::new(pad, bottom.y + 4.0 * s, pw - 2.0 * pad, 20.0 * s)
        };
        let note = if self.route.note > 0.0 {
            Some((wd.recalculated.as_str(), ON_TIME))
        } else if self.route.joined
            && !self.route.lanes.is_empty()
            && !self.route.on_route
            && self.route.off_for > OFF_ROUTE_AFTER
        {
            Some((
                if self.route.off_for < OFF_ROUTE_AFTER + 20.0 {
                    wd.rerouting.as_str()
                } else {
                    wd.off_route.as_str()
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
        match f.stops.first() {
            Some(st) => {
                let name = if n_stops == 1 {
                    format!("{} · {}", st.name.trim(), wd.last_stop)
                } else {
                    st.name.trim().to_string()
                };
                let mut name_row = stop_row;
                if let Some(line) = f.line.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
                    let lw = self.fonts.width(line, 12.5 * s, Weight::Bold) + 12.0 * s;
                    let badge = Rect::new(stop_row.x, stop_row.center().y - 9.0 * s, lw, 18.0 * s);
                    ui.rounded(badge, 4.0 * s, ACCENT);
                    ui.text_in(
                        &mut self.atlas,
                        &self.fonts,
                        line,
                        12.5 * s,
                        Weight::Bold,
                        badge,
                        Align::Center,
                        Color::rgba(18, 14, 8, 1.0),
                    );
                    name_row.x += lw + 7.0 * s;
                    name_row.w -= lw + 7.0 * s;
                }
                ui.text_in(
                    &mut self.atlas,
                    &self.fonts,
                    &name,
                    13.5 * s,
                    Weight::Bold,
                    name_row,
                    Align::Left,
                    TEXT,
                );
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
                parts.push(format!(
                    "{:02}:{:02}",
                    (st.arrival / 3600.0) as i32 % 24,
                    ((st.arrival % 3600.0) / 60.0) as i32
                ));
                let line2 = parts.join("  ·  ");
                let y2 = Rect::new(pad, bottom.y + 24.0 * s, pw - 2.0 * pad, 18.0 * s);
                match note {
                    Some((t, c)) => {
                        ui.text_in(
                            &mut self.atlas,
                            &self.fonts,
                            t,
                            12.5 * s,
                            Weight::Medium,
                            y2,
                            Align::Left,
                            c,
                        );
                    }
                    None => match &jam_note {
                        Some((t, c)) => {
                            ui.text_in(
                                &mut self.atlas,
                                &self.fonts,
                                &format!("{line2}  ·  {t}"),
                                12.5 * s,
                                Weight::Medium,
                                y2,
                                Align::Left,
                                *c,
                            );
                        }
                        None => {
                            ui.text_in(
                                &mut self.atlas,
                                &self.fonts,
                                &line2,
                                12.5 * s,
                                Weight::Medium,
                                y2,
                                Align::Left,
                                TEXT_DIM,
                            );
                        }
                    },
                }
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
                    ui.text_in(
                        &mut self.atlas,
                        &self.fonts,
                        &txt,
                        12.5 * s,
                        Weight::Bold,
                        y2,
                        Align::Right,
                        c,
                    );
                }
            }
            None => {
                if let Some(t) = f
                    .terminus
                    .clone()
                    .filter(|t| has_bottom && !t.trim().is_empty())
                {
                    ui.text_in(
                        &mut self.atlas,
                        &self.fonts,
                        &t,
                        13.0 * s,
                        Weight::Medium,
                        stop_row,
                        Align::Left,
                        TEXT_DIM,
                    );
                }
            }
        }
        if self.sched_e > 0.001 && !f.stops.is_empty() {
            let mut y = bottom.bottom() + 6.0 * s;
            ui.rect(
                Rect::new(pad, bottom.bottom(), pw - 2.0 * pad, 1.0),
                Color::WHITE.alpha(0.06),
            );
            let late = f.delay.unwrap_or(0.0);
            for st in f.stops.iter().take(5) {
                let r = Rect::new(pad, y, pw - 2.0 * pad, 22.0 * s);
                let planned = format!(
                    "{:02}:{:02}",
                    (st.arrival / 3600.0) as i32 % 24,
                    ((st.arrival % 3600.0) / 60.0) as i32
                );
                ui.text_in(
                    &mut self.atlas,
                    &self.fonts,
                    &planned,
                    12.5 * s,
                    Weight::Bold,
                    r,
                    Align::Left,
                    TEXT_DIM,
                );
                ui.text_in(
                    &mut self.atlas,
                    &self.fonts,
                    st.name.trim(),
                    13.0 * s,
                    Weight::Medium,
                    Rect::new(r.x + 46.0 * s, r.y, r.w - 100.0 * s, r.h),
                    Align::Left,
                    TEXT,
                );
                let exp = st.arrival + late;
                let e = format!(
                    "{:02}:{:02}",
                    (exp / 3600.0).rem_euclid(24.0) as i32,
                    ((exp.rem_euclid(3600.0)) / 60.0) as i32
                );
                ui.text_in(
                    &mut self.atlas,
                    &self.fonts,
                    &e,
                    12.5 * s,
                    Weight::Medium,
                    r,
                    Align::Right,
                    if late > 59.0 {
                        LATE
                    } else if late < -59.0 {
                        EARLY
                    } else {
                        TEXT_DIM
                    },
                );
                y += 22.0 * s;
            }
        }

        ui.rounded_border(panel, radius, 1.0_f32.max(s), HAIR);

        let (Some(gpu), device, queue) = (self.gpu.as_mut(), &renderer.device, &renderer.queue)
        else {
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
        let mut all = bg.verts;
        all.extend(dy.verts);
        let n_ui_start = all.len() as u32;
        all.extend(ui.verts);
        gpu.upload(device, queue, 2, &all);
        gpu.upload_atlas(queue, &mut self.atlas);
        let flat = Layer::flat(clip_panel, radius, 1.0);
        let backdrop = Layer::flat(
            clip_panel,
            radius,
            if self.cockpit_display {
                self.opacity
            } else {
                crate::ui::backdrop(self.opacity).min(1.0)
            },
        );
        let mut layers = [flat, map_layer, backdrop];
        for l in layers.iter_mut() {
            l.opacity *= self.shown;
        }
        let roads_n = self.roads.as_ref().map(|r| r.verts as u32).unwrap_or(0);
        let draws = [
            Draw {
                buffer: 2,
                range: 0..n_bg,
                layer: 2,
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
                layer: 1,
                texture: 0,
            },
            // the traffic under the route: parked cars must not cut the line up
            Draw {
                buffer: 2,
                range: n_bg + n_traffic..n_bg + n_world,
                layer: 1,
                texture: 0,
            },
            Draw {
                buffer: 1,
                range: 0..self.route_mesh.verts,
                layer: 1,
                texture: 0,
            },
            Draw {
                buffer: 2,
                range: n_ui_start..all.len() as u32,
                layer: 0,
                texture: 0,
            },
        ];
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
            &layers,
            &draws,
        );
        queue.submit([enc.finish()]);
    }
}
