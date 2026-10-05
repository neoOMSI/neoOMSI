use super::*;

/// How the player's bus is driven, as Omsi.exe watches it for the riders (0x7d5124,
/// 0x7d65d4 - 0x7d6b7f): the longitudinal acceleration eased over a tenth of a second, the
/// lateral one over a second (both weighed down below 1 m/s), and the swings of the first
/// between +0.2 and -0.2 m/s² (a jerky right foot).
#[derive(Debug, Clone, Default)]
pub(in crate::humans) struct RideComfort {
    /// +0x780 and +0x784 (m/s²).
    fast_long: f32,
    slow_lat: f32,
    /// The last swing went up (+0x79c), when (+0x794, ms) and how many came in a row (+0x798).
    up: bool,
    swing_ms: f64,
    swings: u32,
    /// The last hard bend or braking (+0x790, ms).
    hard_ms: f64,
    /// `VehicleInstance::crashes` seen on the previous frame.  A collision is discrete;
    /// its acceleration can be averaged away before the passenger tick sees it.
    crashes: u32,
}

impl RideComfort {
    /// One frame of the bus (`speed` forward and the body's acceleration `lat` to the right
    /// and `long` forward, m/s and m/s²): how much this frame upsets the riders - 0, 0.05
    /// for the fifth and every further swing of the throttle and brake less than 4 s apart,
    /// 0.1 for a bend taken at over 3 m/s² or braking or pulling away at over 5 m/s² (once
    /// a second at most).  A newly reported collision is also a 0.15 jolt, so an impact
    /// cannot disappear when the physics acceleration is averaged over a frame.
    pub(in crate::humans) fn step(
        &mut self,
        dt: f32,
        now_ms: f64,
        speed: f32,
        lat: f32,
        long: f32,
        crashes: u32,
    ) -> f32 {
        let w = speed.abs().min(1.0);
        let kf = (10.0 * dt).min(0.5);
        let ks = dt.min(0.5);
        self.fast_long = w * long * kf + (1.0 - kf) * self.fast_long;
        self.slow_lat = w * lat * ks + (1.0 - ks) * self.slow_lat;
        let mut k: f32 = 0.0;
        if self.fast_long > 0.2 && !self.up {
            if now_ms < self.swing_ms + 4000.0 {
                self.swings += 1;
                if self.swings > 4 {
                    k = 0.05;
                }
            } else {
                self.swings = 0;
            }
            self.swing_ms = now_ms;
            self.up = true;
        } else if self.fast_long < -0.2 && self.up {
            // (back within half a second: no swing, the count starts again)
            if now_ms < self.swing_ms + 4000.0 && now_ms > self.swing_ms + 500.0 {
                self.swings += 1;
                if self.swings > 4 {
                    k = 0.05;
                }
            } else {
                self.swings = 0;
            }
            self.swing_ms = now_ms;
            self.up = false;
        }
        if self.slow_lat.abs() > 3.0 || self.fast_long.abs() > 5.0 {
            if self.hard_ms + 1000.0 < now_ms {
                k = 0.1;
            }
            self.hard_ms = now_ms;
        }
        if crashes > self.crashes {
            k = k.max(0.15);
        }
        self.crashes = crashes;
        k
    }
}

/// Where a rider's complaints about the driving come (the human's constructor, 0x625a3f):
/// the first below 0.1, the second from 0.2 to 0.4, the third (and off at the next stop)
/// from 0.5 to 0.8, for `r` three draws from 0..1.
pub(in crate::humans) fn bad_ride_thresholds(r: [f32; 3]) -> [f32; 3] {
    let a = 0.1 * r[0];
    [a, 0.1 + a.max(0.1) + 0.2 * r[1], 0.5 + 0.3 * r[2]]
}

/// The complaint a rider says as the ride's toll `x` reaches their next threshold
/// (0x7d6a22 - 0x7d6b7f; the worst first, each only once): 1, 2, 3 or none.
pub(in crate::humans) fn bad_ride_complaint(
    x: f32,
    said: Complaint,
    at: [f32; 3],
) -> Option<Complaint> {
    if at[2] <= x && said < Complaint::Leave {
        Some(Complaint::Leave)
    } else if at[1] <= x && said < Complaint::Strong {
        Some(Complaint::Strong)
    } else if at[0] <= x && said < Complaint::Mild {
        Some(Complaint::Mild)
    } else {
        None
    }
}

impl Humans {
    /// The riders of the player's bus feel how it is driven (0x7d6964 - 0x7d6b7f): every
    /// jolt (`RideComfort::step`) takes the toll of the ride `(1 - x) * k` up for everybody
    /// walking or sitting in it, and whoever reaches a threshold says so (TooBad_A, _B, _C
    /// of the ticket pack's voices) - the third time getting off at the next stop. The
    /// toll eases off by 0.2 a kilometre (`pax_tick`). OMSI's passengers did this; here
    /// they never said a word about the driving (#862, #873).
    pub(in crate::humans) fn ride_comfort(
        &mut self,
        dt: f32,
        bus: Option<&VehicleInstance>,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
        world: &World,
    ) {
        let Some(v) = bus else { return };
        if dt <= 0.0 || self.avatar_only {
            return;
        }
        let a = v.physics.a_trans;
        let k = self
            .comfort
            .step(dt, self.time * 1000.0, v.physics.speed, a.x, a.y, v.crashes);
        if k <= 0.0 {
            return;
        }
        for i in 0..self.people.len() {
            if self.people[i].remote || self.people[i].puppet.is_some() {
                continue;
            }
            let Some(p) = self.pax(i) else { continue };
            if p.bus != Some(BusId::Player)
                || p.inside != Some(BusId::Player)
                || !matches!(
                    p.task,
                    Task::InBusToPlace | Task::InBusToExit | Task::SittingInBus
                )
            {
                continue;
            }
            if p.bad_at[2] <= 0.0 {
                let r = [
                    self.rand_f() as f32,
                    self.rand_f() as f32,
                    self.rand_f() as f32,
                ];
                self.pax_mut(i).unwrap().bad_at = bad_ride_thresholds(r);
            }
            let p = self.pax_mut(i).unwrap();
            p.discomfort += (1.0 - p.discomfort) * k;
            let Some(c) = bad_ride_complaint(p.discomfort, p.complaint, p.bad_at) else {
                continue;
            };
            p.complaint = c;
            if debug_pax() {
                log::info!(
                    "t={:.1} pax {} complains about the driving ({c}, toll {:.2})",
                    self.time,
                    self.people[i].label(),
                    self.pax(i).unwrap().discomfort
                );
            }
            match c {
                Complaint::Mild => {
                    self.say_ex(i, "TooBad_A", true);
                }
                Complaint::Strong => {
                    self.say_ex(i, "TooBad_B", true);
                }
                _ => {
                    self.say_ex(i, "TooBad_C", true);
                    // OMSI's RL_PassDlg_TooBad_C appears when the complaint becomes
                    // serious enough for passengers to leave.  The earlier complaints
                    // are heard, but do not interrupt the driver with a HUD warning.
                    self.message = Some("Passengers want to leave because of your driving.".into());
                    self.set_task(i, Task::InBusToExit, buses, bus_ix, world);
                }
            }
        }
    }

    /// The greeting or complaint stepping into the player's bus (0x62bf2d - 0x62c43c).
    pub(in crate::humans) fn greet_or_complain(&mut self, i: usize, bn: &BusNow) {
        let Some((whinge, chat)) = self.tickets.as_ref().map(|t| (t.whinge_prop, t.chattiness))
        else {
            return;
        };
        let air = bn.air;
        let mut complaint_seen = false;
        let mut code = 0u8;
        // too dark: the saloon light under half and dusk outside
        if bn.interior < 0.5 {
            let r = self.rand_f() as f32;
            if air.brightness < 0.2 + 0.3 * r {
                complaint_seen = true;
                if (self.rand_f() as f32) < whinge {
                    code = 1;
                }
            }
        }
        if let Some(t) = air.temp {
            let out = air.outside;
            let r = (self.rand() % 10) as f32 + 25.0;
            let hot = if t <= r {
                false
            } else {
                let r5 = (self.rand() % 5) as f32 + 3.0;
                t > r5 + out
            };
            let hot = hot || {
                let r = (self.rand() % 10) as f32;
                out * 0.5 + r + 20.0 < t && t < 25.0
            };
            if hot {
                complaint_seen = true;
                if code == 0 && (self.rand_f() as f32) < whinge {
                    code = if air.rel_hum <= 0.9 + 0.1 * self.rand_f() as f32 {
                        3
                    } else {
                        5
                    };
                }
            }
            let r = (self.rand() % 10) as f32 + 8.0;
            let cold = if t >= r {
                let r = (self.rand() % 10) as f32;
                t < (out - r) - 10.0
            } else {
                let r5 = (self.rand() % 5) as f32;
                t < out + 5.0 + r5 || {
                    let r = (self.rand() % 10) as f32;
                    t < (out - r) - 10.0
                }
            };
            if cold {
                complaint_seen = true;
                if code == 0 && (self.rand_f() as f32) < whinge {
                    code = 4;
                }
            }
        }
        if self.delay > 300.0 {
            complaint_seen = true;
            if code == 0 && (self.rand_f() as f32) < whinge {
                code = 2;
            }
        }
        let k = 1 + self.rand() % 2;
        match code {
            1 => {
                self.say_ex(i, &format!("TooDark_{k}"), true);
            }
            2 => {
                self.say_ex(i, &format!("TooLate_{k}"), true);
            }
            3 => {
                self.say_ex(i, &format!("TooHot_{k}"), true);
            }
            4 => {
                self.say_ex(i, &format!("TooCold_{k}"), true);
            }
            5 => {
                self.say_ex(i, "TooWet_1", true);
            }
            _ => {
                if (self.rand_f() as f32) < chat {
                    let h = (self.time_of_day.rem_euclid(86_400.0) / 3600.0).floor() as i32;
                    let daypart = if (3..=10).contains(&h) {
                        1
                    } else if (18..=23).contains(&h) {
                        2
                    } else {
                        0
                    };
                    let k = if daypart == 0 {
                        self.rand() % 2
                    } else {
                        self.rand() % 3
                    };
                    if k < 2 {
                        self.say_ex(i, &format!("Hello_{}", k + 1), false);
                    } else if daypart == 1 {
                        self.say_ex(i, "GoodMorning_1", false);
                    } else {
                        self.say_ex(i, "GoodEvening_1", false);
                    }
                }
            }
        }
        // OMSI's rating: people who stepped in, and those content
        self.stepped_in += 1;
        if !complaint_seen {
            self.content += 1;
        }
    }
}
