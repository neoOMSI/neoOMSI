use super::*;

pub(super) fn congestion_on(net: &Network, traffic: &Network, c: &HashMap<usize, f32>) -> HashMap<usize, f32> {
    let mut out = HashMap::new();
    for (&l, &v) in c {
        let Some(lane) = traffic.lanes.get(l) else {
            continue;
        };
        let Some(key) = lane.key else { continue };
        if let Some(cands) = net.by_key.get(&key) {
            for &g in cands {
                if net.lanes[g].reversed == lane.reversed {
                    out.insert(g, v);
                }
            }
        }
    }
    out
}

impl Navigator {
    pub(super) fn follow(&mut self, f: &NavFrame) {
        let global = self.global.clone();
        let Some(net) = global.as_deref().or(f.traffic.map(|t| t.net())) else {
            return;
        };
        let r = &mut self.route;
        if r.lanes.is_empty() {
            let Some(stop) = f.stops.first() else { return };
            r.retry_in -= f.dt;
            if r.retry_in > 0.0 {
                return;
            }
            r.retry_in = REROUTE_EVERY * 2.0;
            let targets: Vec<usize> = lanes_near(net, stop.position.truncate(), 40.0)
                .into_iter()
                .filter(|&i| {
                    net.lanes[i].kind == LaneKind::Street
                        && net.lanes[i]
                        .nearest_point(stop.position)
                        .map(|p| p.1 < 20.0)
                        .unwrap_or(false)
                })
                .collect();
            if let Some((mut path, k)) = way_back(net, f.bus, f.heading, &targets, 30_000.0) {
                path.push(targets[k]);
                log::info!(
                    "navigator: no route of the trip here yet; {} lanes to the next stop '{}'",
                    path.len(),
                    stop.name.trim()
                );
                r.lanes = path;
                r.progress = 0;
                r.s = 0.0;
                r.version += 1;
                r.provisional = true;
            }
            return;
        }
        r.note = (r.note - f.dt).max(0.0);
        let stop_at = f.stops.first().and_then(|st| {
            (r.progress..r.lanes.len().min(r.progress + 1500)).find(|&k| {
                net.lanes
                    .get(r.lanes[k])
                    .and_then(|l| l.nearest_point(st.position))
                    .map(|p| p.1 < 25.0)
                    .unwrap_or(false)
            })
        });
        let (from, to) = match (r.joined, stop_at) {
            (false, Some(k)) if r.approach => {
                (r.progress.saturating_sub(3), (k + 2).min(r.lanes.len()))
            }
            (false, Some(k)) => (
                k.saturating_sub(40).max(r.progress),
                (k + 2).min(r.lanes.len()),
            ),
            _ => (
                r.progress.saturating_sub(3),
                (r.progress + 60).min(r.lanes.len()),
            ),
        };
        let mut best: Option<(usize, f32, f64)> = None;
        for k in from..to {
            let Some(l) = net.lanes.get(r.lanes[k]) else {
                continue;
            };
            let Some((s, d)) = l.nearest_point(f.bus) else {
                continue;
            };
            if d > 16.0 {
                continue;
            }
            let (_, h) = l.at(s);
            if angle_diff(f.heading, h as f64).abs() > 100.0 {
                continue;
            }
            let score = d
                + if k < r.progress { 4.0 } else { 0.0 }
                + (k.saturating_sub(r.progress) as f64) * 0.05;
            if best.map(|b| score < b.2).unwrap_or(true) {
                best = Some((k, s, score));
            }
        }
        match best {
            Some((k, s, _)) => {
                if k != r.progress {
                    r.version += 1;
                }
                r.progress = k;
                r.s = s;
                r.on_route = true;
                r.joined = true;
                r.off_for = 0.0;
            }
            None => {
                r.on_route = false;
                r.off_for += f.dt;
            }
        }
        if r.on_route || (r.joined && r.off_for < OFF_ROUTE_AFTER) {
            return;
        }
        r.retry_in -= f.dt;
        if r.retry_in > 0.0 {
            return;
        }
        r.retry_in = REROUTE_EVERY;
        let base = r.progress.min(r.lanes.len() - 1);
        let (lo, hi) = match stop_at {
            Some(k) => (k.saturating_sub(150).max(base), k + 1),
            None => (base, (base + 120).min(r.lanes.len())),
        };
        let way = way_back(
            net,
            f.bus,
            f.heading,
            &r.lanes[lo..hi],
            if r.joined { 6000.0 } else { 30_000.0 },
        );
        if way.is_none() && ::legacy_config::env::var_os("OMSI_DEBUG_NAV").is_some() {
            let info: Vec<_> = r.lanes[lo..hi]
                .iter()
                .map(|&l| {
                    (
                        l,
                        net.lanes[l].kind,
                        net.lanes[l].name.clone(),
                        net.lanes.iter().filter(|x| x.next.contains(&l)).count(),
                        net.lanes[l].start(),
                    )
                })
                .collect();
            log::info!(
                "navigator: off the route for {:.1} s and no way back found; targets {info:?}",
                r.off_for
            );
        }
        let max = if r.joined { 6000.0 } else { 30_000.0 };
        let way = way.map(|(p, j)| (p, lo + j)).or_else(|| {
            let k = stop_at?;
            let st = f.stops.first()?;
            let (_, h) = net.lanes[r.lanes[k]].nearest_point(st.position).map(|(s, _)| net.lanes[r.lanes[k]].at(s))?;
            let near: Vec<usize> = lanes_near(net, st.position.truncate(), 40.0)
                .into_iter()
                .filter(|&i| {
                    let l = &net.lanes[i];
                    l.kind == LaneKind::Street
                        && l.nearest_point(st.position).map(|(s, d)| d < 20.0 && angle_diff(l.at(s).1 as f64, h as f64).abs() < 60.0).unwrap_or(false)
                })
                .collect();
            let (mut path, j) = way_back(net, f.bus, f.heading, &near, max)?;
            path.push(near[j]);
            log::info!("navigator: the route before stop '{}' cannot be reached; led onto the road past it", st.name.trim());
            Some((path, k + 1))
        })
            .or_else(|| {
                let k = stop_at?;
                let hi = (k + 120).min(r.lanes.len());
                (k + 1 < hi).then_some(())?;
                way_back(net, f.bus, f.heading, &r.lanes[k + 1..hi], max).map(|(p, j)| (p, k + 1 + j))
            });
        if let Some((path, join)) = way {
            let rest = r.lanes[join.min(r.lanes.len())..].to_vec();
            log::info!(
                "navigator: {} of {} lanes joins the route {} lanes on{}",
                if r.joined {
                    "a way back"
                } else {
                    "the way to the route"
                },
                path.len(),
                join,
                stop_at
                    .map(|k| format!(" (the next stop is on route lane {k})"))
                    .unwrap_or_default()
            );
            let mut lanes = path;
            lanes.extend(rest);
            r.s = lanes
                .first()
                .and_then(|&l| net.lanes.get(l))
                .and_then(|l| l.nearest_point(f.bus))
                .map(|p| p.0)
                .unwrap_or(0.0);
            r.lanes = lanes;
            r.progress = 0;
            r.version += 1;
            if r.joined {
                r.note = 4.0;
                r.on_route = true;
                r.off_for = 0.0;
            } else {
                r.approach = true;
            }
        }
    }

    pub(super) fn update_congestion(&mut self, f: &NavFrame) {
        self.congestion_t -= f.dt;
        if self.congestion_t > 0.0 {
            return;
        }
        let step = 0.25f32;
        self.congestion_t = step;
        let Some(t) = f.traffic else { return };
        let mut by_lane: HashMap<usize, (f32, u32)> = HashMap::new();
        for c in t.cars() {
            if c.gone || (c.vehicle.position - f.bus).truncate().length() > 1200.0 {
                continue;
            }
            let e = by_lane.entry(c.state.lane).or_insert((0.0, 0));
            e.0 += c.state.speed.max(0.0);
            e.1 += 1;
        }
        let k = ease(step, 4.0) as f32;
        for v in self.congestion.values_mut() {
            *v -= *v * k;
        }
        for (lane, (sum, n)) in by_lane {
            let Some(l) = t.net().lanes.get(lane) else {
                continue;
            };
            let expected = (l.speed_limit_kmh.clamp(20.0, 70.0) / 3.6) * 0.75;
            let slow = (1.0 - (sum / n as f32) / expected).clamp(0.0, 1.0);
            let full = (n as f32 * 7.5 / l.length().max(25.0)).min(1.0);
            let light = 0.22 + 0.33 * full;
            let jam = slow * (n as f32 / 3.0).min(1.0);
            let score = if jam > 0.2 {
                light.max(0.3 + 0.7 * jam)
            } else {
                light
            };
            let v = self.congestion.entry(lane).or_insert(0.0);
            *v += (score - *v) * k;
        }
        self.congestion.retain(|_, v| *v > 0.03);
        let global = self.global.clone();
        let jam = match global.as_deref() {
            Some(g) => congestion_on(g, t.net(), &self.congestion),
            None => self.congestion.clone(),
        };
        let net = global.as_deref().unwrap_or(t.net());
        let r = &self.route;
        let mut route_jam = HashMap::new();
        let mut cost = 0.0;
        for &l in r.lanes.iter().skip(r.progress).take(600) {
            let Some(&c) = jam.get(&l) else { continue };
            route_jam.insert(l, c);
            if let Some(lane) = net.lanes.get(l) {
                if c > 0.6 {
                    let v_free = lane.speed_limit_kmh.clamp(20.0, 70.0) / 3.6;
                    let v = (v_free * (1.0 - c)).max(1.2);
                    cost += lane.length() / v - lane.length() / v_free;
                }
            }
        }
        self.jam_cost = cost;
        let changed = route_jam.len() != self.route_jam.len()
            || route_jam.iter().any(|(l, c)| {
            self.route_jam
                .get(l)
                .map(|o| level(*o) != level(*c))
                .unwrap_or(true)
        });
        self.route_jam = route_jam;
        if changed {
            self.jam_version += 1;
        }
    }

    pub(super) fn turn_ahead(&self, net: &Network) -> Option<(i32, f32, f64, Option<String>)> {
        let r = &self.route;
        if !r.on_route {
            return None;
        }
        let mut acc = -(r.s as f64);
        let mut prev_end: Option<f32> = None;
        for (j, &l) in r.lanes.iter().enumerate().skip(r.progress) {
            let lane = net.lanes.get(l)?;
            let len = lane.length();
            if acc > 1500.0 {
                break;
            }
            let (h0, h1) = (lane.start_heading(), lane.end_heading());
            let mut d = ::traffic::wrap_deg(h1 - h0);
            if let Some(pe) = prev_end {
                d += ::traffic::wrap_deg(h0 - pe);
            }
            if d.abs() > 35.0 && (len < 60.0 || d.abs() > 70.0) && acc + len as f64 > 0.0 {
                let dir = if d.abs() > 150.0 {
                    2
                } else if d > 0.0 {
                    1
                } else {
                    -1
                };
                let street = r
                    .lanes
                    .iter()
                    .skip(j + 1)
                    .take(4)
                    .chain(std::iter::once(&l))
                    .find_map(|&x| self.street_of(x))
                    .map(str::to_string);
                return Some((dir, d.abs(), acc.max(0.0), street));
            }
            prev_end = Some(h1);
            acc += len as f64;
        }
        None
    }

    pub(super) fn street_of(&self, lane: usize) -> Option<&str> {
        self.global.as_ref()?;
        let st = self.streets.as_deref()?;
        let i = *st.of_lane.get(lane)?;
        st.names.get(i as usize).map(String::as_str)
    }

    pub(super) fn route_distance(&self, net: &Network, stop: DVec3) -> Option<f64> {
        let r = &self.route;
        let mut acc = -(r.s as f64);
        for &l in r.lanes.iter().skip(r.progress) {
            let lane = net.lanes.get(l)?;
            if let Some((s, d)) = lane.nearest_point(stop) {
                if d < 25.0 && (acc + s as f64) >= -5.0 {
                    return Some((acc + s as f64).max(0.0));
                }
            }
            acc += lane.length() as f64;
            if acc > 30_000.0 {
                break;
            }
        }
        None
    }
}

