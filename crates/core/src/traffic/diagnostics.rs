//! Diagnostics and capture: trace events, rolling capture and health reports.

use super::*;

impl Traffic {

    /// Start the first automatic failure capture, persisted to `path` when a trigger fires.
    pub fn enable_capture(&mut self, path: std::path::PathBuf, capacity: usize) {
        let header = TraceHeader {
            trace_version: TRACE_VERSION,
            source_revision: env!("CARGO_PKG_VERSION").to_string(),
            platform: std::env::consts::OS.to_string(),
            seed: self.rng,
            tick_hz: 50.0,
            network_version: NetworkVersion(self.lanes_generation),
            input_digest: 0,
        };
        self.capture = Some((path, Capture::new(header, capacity)));
    }


    /// Forward one typed lifecycle event to the rolling capture (if enabled).
    pub(crate) fn emit_trace(&mut self, event: TraceEvent) {
        if let Some((_, cap)) = self.capture.as_mut() {
            cap.emit(event);
        }
    }


    /// Sample one tick into the rolling capture and persist it on the first trigger.
    pub(crate) fn sample_capture(&mut self) {
        let network_version = NetworkVersion(self.lanes_generation);
        let mut stationary_without_reason = false;
        let vehicles: Vec<VehicleSnapshot> = self
            .cars
            .iter()
            .map(|c| {
                let slow = c.state.speed < 0.05;
                let no_reason =
                    c.why.0.is_none() && c.junction_why.is_empty() && c.holding.is_none();
                if slow && no_reason {
                    stationary_without_reason = true;
                }
                VehicleSnapshot {
                    id: c.id,
                    lane: LaneId(c.state.lane),
                    s: c.state.s,
                    speed: c.state.speed,
                    realized_speed: c.state.realized_speed,
                    accel: c.state.acc,
                    emergency: c.state.emergency,
                    reconciled: c.state.reconciled,
                    front: c.state.front,
                    rear: c.state.rear,
                    junction_state: c.junction_state,
                    junction_blocker: self.junctions.blocked_by(c.id),
                    service_phase: c
                        .bus
                        .as_ref()
                        .map(|b| b.state.phase)
                        .unwrap_or(ServicePhase::EnRoute),
                    berth_owner: c
                        .bus
                        .as_ref()
                        .and_then(|b| b.state.berth)
                        .and_then(|h| self.services.berth_owner(h.stop, h.occurrence))
                        .filter(|owner| *owner != c.id),
                    service_stop: c.bus.as_ref().and_then(|b| b.stops.front()).map(|s| s.stop),
                    maneuver_phase: if let Some(p) = c.maneuver.passing {
                        if p.aborted {
                            ManeuverPhase::PassingAbort
                        } else {
                            ManeuverPhase::Passing
                        }
                    } else if c.maneuver.park.is_some() {
                        ManeuverPhase::Parking
                    } else if c.maneuver.pull_out > 0.0 {
                        ManeuverPhase::PullOut
                    } else {
                        ManeuverPhase::Idle
                    },
                    maneuver_target: c.state.change.map(|ch| LaneId(ch.to)),
                    lifecycle: Lifecycle::Active,
                    constraints: if slow && no_reason {
                        vec![Reason::Unknown(0)]
                    } else {
                        Vec::new()
                    },
                    binding: if slow && no_reason {
                        Some(Reason::Unknown(0))
                    } else {
                        None
                    },
                }
            })
            .collect();
        let tick = self.time.max(0.0) as u64;
        let snap = TickSnapshot {
            tick,
            sim_time: self.time as f64,
            network_version,
            vehicles,
        };
        let Some((path, cap)) = self.capture.as_mut() else {
            return;
        };
        cap.push_tick(snap);
        if stationary_without_reason {
            cap.note_trigger(CaptureTrigger::StationaryWithoutReason);
        }
        if cap.captured() && !self.capture_written {
            self.capture_written = true;
            write_capture(path, cap);
        }
    }


    /// What holds each car that has stood for over a minute (the offscreen traffic health
    /// report): why, its blinker, its light and the lane.
    pub fn stuck_report(&self) -> Vec<String> {
        self.cars
            .iter()
            .filter(|c| c.stopped > 60.0 || (c.stopped > 15.0 && c.state.signal != 0 && c.yielding))
            .map(|c| {
                let st = &c.state;
                let light = self.way_lanes(st, 60.0).into_iter().find_map(|(l, d)| {
                    self.net.lanes[l].traffic_light.and_then(|(ci, li)| self.lights.get(ci).map(|ctl| format!("light {ci}/{li} state {} at {d:.1} (time {:.0}, held {}, cycle {:.0}, phases {:?}, stops {:?})", ctl.state(li), ctl.time, ctl.held, ctl.cycle, ctl.lights, ctl.stops)))
                });
                format!(
                    "car {} {} stood {:.0} s: why {:?} blinker {} yielding {} light_hold {} lane {} ({}) s {:.1}/{:.1} next {:?} {} lead {:?} bus {:?} pos ({:.1}, {:.1}) junction {} [geo_block {:?} squeeze {:?} wait_at {:?} held {} start_timer {:.2} accel_cap {:?} crawl {:.1} passing {} park {} pull_out {:.1} acc {:.2}]",
                    c.id,
                    c.vehicle.ty.def.path.file_stem().unwrap_or_default().to_string_lossy(),
                    c.stopped,
                    c.why,
                    st.signal,
                    c.yielding,
                    c.light_hold,
                    st.lane,
                    self.net.lanes[st.lane].name,
                    st.s,
                    self.net.lanes[st.lane].length(),
                    st.upcoming().take(3).collect::<Vec<_>>(),
                    light.unwrap_or_default(),
                    c.lead_info,
                    c.bus.as_ref().map(|b| b.state.phase),
                    c.vehicle.position.x,
                    c.vehicle.position.y,
                    c.junction_why,
                    c.geo_block,
                    c.squeeze,
                    c.wait_at,
                    c.held,
                    st.start_timer,
                    st.accel_cap,
                    c.crawl,
                    c.maneuver.passing.is_some(),
                    c.maneuver.park.is_some(),
                    c.maneuver.pull_out,
                    st.acc
                )
            })
            .collect()
    }


    /// `OMSI_CHECK_OVERLAP`: every AI vehicle whose body has got into another's or into a
    /// player's bus (by more than 20 cm), once per pair and 10 s, with what each was doing.
    pub(crate) fn check_overlaps(&mut self, player: Option<PlayerBox>, others: &[(u32, PlayerBox)]) {
        static SEEN: std::sync::OnceLock<parking_lot::Mutex<HashMap<(u64, u64), f32>>> =
            std::sync::OnceLock::new();
        let seen = SEEN.get_or_init(|| parking_lot::Mutex::new(HashMap::new()));
        let feet = self.footprints();
        let boxes: Vec<(u64, Footprint)> = player
            .iter()
            .map(|b| (u64::MAX, *b))
            .chain(others.iter().map(|(id, b)| (u64::MAX - 1 - *id as u64, *b)))
            .map(|(id, (c, h, hl, hw, v))| {
                let hr = h.to_radians();
                (
                    id,
                    Footprint {
                        car: usize::MAX,
                        center: c.truncate(),
                        fwd: DVec2::new(hr.sin(), hr.cos()),
                        right: DVec2::new(hr.cos(), -hr.sin()),
                        half_len: hl as f64,
                        half_w: hw as f64,
                        speed: v,
                        z: c.z,
                    },
                )
            })
            .collect();
        let why = |c: &AiCar| {
            format!(
                "{} v {:.1} lane {} s {:.1} lat {:.2} why {:?} passing {} change {}",
                c.vehicle
                    .ty
                    .def
                    .path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy(),
                c.state.speed,
                c.state.lane,
                c.state.s,
                c.state.lateral,
                c.why,
                c.maneuver.passing.is_some(),
                c.state.change.is_some()
            )
        };
        for (a, fa) in feet.iter().enumerate() {
            let ca = &self.cars[fa.car];
            let hit = |other: u64, fb: &Footprint, desc: String| {
                if (fa.z - fb.z).abs() > 3.0 || !fa.overlaps(fb, -0.2) {
                    return;
                }
                let key = (ca.id.get().min(other), ca.id.get().max(other));
                let mut m = seen.lock();
                if m.get(&key).is_some_and(|t| self.time - *t < 10.0) {
                    return;
                }
                m.insert(key, self.time);
                log::info!(
                    "t={:.1}: OVERLAP car {} ({}) with {} at ({:.1}, {:.1})",
                    self.time,
                    ca.id,
                    why(ca),
                    desc,
                    fa.center.x,
                    fa.center.y
                );
            };
            for fb in feet.iter().skip(a + 1) {
                if fb.car == fa.car {
                    continue;
                }
                let cb = &self.cars[fb.car];
                hit(cb.id.get(), fb, format!("car {} ({})", cb.id, why(cb)));
            }
            for (id, fb) in &boxes {
                hit(
                    *id,
                    fb,
                    format!("player box {} v {:.1}", u64::MAX - id, fb.speed),
                );
            }
        }
    }

}
