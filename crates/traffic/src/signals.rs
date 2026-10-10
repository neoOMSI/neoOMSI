/// A `[traffic_light_stop]` / `[traffic_light_jump]` of a light program (`TAmpelStop`:
/// ampel, time, jumptime, ifAnf). When the cycle clock reaches `time` and the condition
/// holds, the clock waits there (stop) or continues at `jump_to` (jump). With `if_request`
/// set the condition is "nobody is asking at `light`": the stock railway crossings keep the
/// road green at 1 s until a train approaches, the bus loop of Heerstraße skips its bus
/// phase when no bus is waiting, the depot gates jump over their phases when nothing comes.
/// Without it the condition is "somebody is asking": a gate stays green while buses keep
/// coming, a barrier stays closed while the train is still there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightStop {
    pub light: usize,
    pub time: f32,
    pub if_request: bool,
    pub jump_to: Option<f32>,
}

/// How far before its stop line a vehicle asks a light for green when the program gives no
/// `[approachdist]` (m): about four seconds at town speed.
pub const DEFAULT_APPROACH: f32 = 50.0;

/// Traffic light program of a crossing object instance (`TAmpelGroup`): its lights'
/// phases and one cycle clock that runs on game time. The clock is state, not a function
/// of the time of day, because the program may wait at a stop point or jump.
#[derive(Debug, Clone)]
pub struct TrafficLightController {
    /// Per light: phases (state, duration).
    pub lights: Vec<Vec<(i32, f32)>>,
    /// Cycle length (`[traffic_lights_group]`); 0 = the longest light's phases.
    pub cycle: f32,
    pub offset: f32,
    /// `[approachdist]` per light.
    pub approach: Vec<Option<f32>>,
    pub stops: Vec<LightStop>,
    /// Position in the cycle (s).
    pub time: f64,
    /// Per light: a vehicle (or a pedestrian) is asking for it this frame.
    pub request: Vec<bool>,
    /// The clock waits at a stop point.
    pub held: bool,
    /// A stop point the clock has just been let past without moving (it is not asked again
    /// at the same instant).
    passed: Option<usize>,
    /// Backwards jumps already used in this cycle. They may extend a phase once, but must
    /// not restart it again until the cycle wraps.
    rewound: Vec<usize>,
    started: bool,
}

impl TrafficLightController {
    pub fn new(lights: Vec<Vec<(i32, f32)>>, cycle: f32) -> TrafficLightController {
        let n = lights.len();
        TrafficLightController {
            lights,
            cycle,
            offset: 0.0,
            approach: vec![None; n],
            stops: Vec::new(),
            time: 0.0,
            request: vec![false; n],
            held: false,
            passed: None,
            rewound: Vec::new(),
            started: false,
        }
    }

    /// From the `[traffic_light]` program of a crossing object: (per light: name, phases
    /// as (state, seconds), `[approachdist]`), the cycle, the stop and jump points.
    pub fn from_program(
        lights: Vec<(Vec<(i32, f32)>, Option<f32>)>,
        cycle: Option<f32>,
        stops: &[[f32; 3]],
        jumps: &[[f32; 4]],
    ) -> TrafficLightController {
        let approach = lights.iter().map(|l| l.1).collect();
        let mut c = TrafficLightController::new(
            lights.into_iter().map(|l| l.0).collect(),
            cycle.unwrap_or(0.0),
        );
        c.approach = approach;
        for s in stops {
            c.stops.push(LightStop {
                light: s[0].max(0.0) as usize,
                time: s[1],
                if_request: s[2] > 0.5,
                jump_to: None,
            });
        }
        for j in jumps {
            c.stops.push(LightStop {
                light: j[0].max(0.0) as usize,
                time: j[1],
                if_request: j[2] > 0.5,
                jump_to: Some(j[3]),
            });
        }
        c
    }

    /// Length of the cycle in seconds.
    pub fn cycle_len(&self) -> f64 {
        if self.cycle > 0.0 {
            return self.cycle as f64;
        }
        self.lights
            .iter()
            .map(|p| p.iter().map(|x| x.1).sum::<f32>())
            .fold(0.0f32, f32::max)
            .max(1.0) as f64
    }

    /// Set the clock from the time of day (s) the first time the program runs: crossings
    /// with the same cycle length then run in step, as a coordinated street would.
    pub fn start(&mut self, day_time: f64) {
        if !self.started {
            self.time = (day_time + self.offset as f64).rem_euclid(self.cycle_len());
            self.started = true;
        }
    }

    /// Request distance of light `i` (m).
    pub fn approach_dist(&self, i: usize) -> f32 {
        self.approach
            .get(i)
            .copied()
            .flatten()
            .unwrap_or(DEFAULT_APPROACH)
    }

    /// Run the cycle clock on by `dt` seconds of game time, honouring the stop and jump
    /// points with this frame's requests (`request`, set by the caller before).
    pub fn advance(&mut self, dt: f32) {
        let cycle = self.cycle_len();
        let mut left = dt.max(0.0) as f64;
        self.held = false;
        let advance_clock = |this: &mut Self, move_by: f64| {
            if move_by > 0.0 && this.time + move_by >= cycle - 1e-6 {
                this.rewound.clear();
            }
            this.time = (this.time + move_by).rem_euclid(cycle);
        };
        // a handful of points per frame at most (a jump may land just before another one)
        for _ in 0..16 {
            let mut best: Option<(usize, f64)> = None;
            for (k, p) in self.stops.iter().enumerate() {
                if self.rewound.contains(&k) {
                    continue;
                }
                let d = (p.time as f64 - self.time).rem_euclid(cycle);
                let d = if d > cycle - 1e-6 { 0.0 } else { d };
                if d < 1e-6 && self.passed == Some(k) {
                    continue;
                }
                if d <= left && best.map(|b| d < b.1).unwrap_or(true) {
                    best = Some((k, d));
                }
            }
            let Some((k, d)) = best else {
                if left > 0.0 {
                    self.passed = None;
                }
                advance_clock(self, left);
                return;
            };
            if d > 1e-6 {
                self.passed = None;
            }
            advance_clock(self, d);
            left -= d;
            let p = self.stops[k];
            let asked = self.request.get(p.light).copied().unwrap_or(false);
            let active = if p.if_request { !asked } else { asked };
            if !active {
                self.passed = Some(k);
                continue;
            }
            match p.jump_to {
                Some(to) => {
                    let target = (to as f64).rem_euclid(cycle);
                    if to <= p.time + 1e-6 {
                        if !self.rewound.contains(&k) {
                            self.rewound.push(k);
                        }
                    }
                    self.time = target;
                    self.passed = Some(k);
                    if left <= 0.0 {
                        return;
                    }
                }
                None => {
                    self.time = p.time as f64;
                    self.held = true;
                    self.passed = None;
                    return;
                }
            }
        }
    }

    /// State of light `i` at position `x` of the cycle (s).
    pub fn state_at(&self, i: usize, x: f64) -> i32 {
        let phases = match self.lights.get(i) {
            Some(p) if !p.is_empty() => p,
            _ => return 6,
        };
        let mut x = x.rem_euclid(self.cycle_len()) as f32;
        for (state, dur) in phases {
            if x < *dur {
                return *state;
            }
            x -= dur;
        }
        // after the last phase its state holds until the cycle starts again
        phases.last().map(|p| p.0).unwrap_or(0)
    }

    /// Current state of light `i` (the `TrafficLightPhase` value of its lamps).
    pub fn state(&self, i: usize) -> i32 {
        self.state_at(i, self.time)
    }

    /// Seconds until light `i` next shows another state, if the clock keeps running (None
    /// within a whole cycle).
    pub fn time_to_change(&self, i: usize) -> Option<f32> {
        let now = self.state(i);
        let cycle = self.cycle_len();
        let mut t = 0.25;
        while t <= cycle {
            if self.state_at(i, self.time + t) != now {
                return Some(t as f32);
            }
            t += 0.25;
        }
        None
    }

    /// Seconds until light `i` lets vehicles go (0 while it does), if the clock keeps
    /// running; None when it never does within a cycle.
    pub fn time_until_go(&self, i: usize) -> Option<f32> {
        if Self::allows_go(self.state(i)) {
            return Some(0.0);
        }
        let cycle = self.cycle_len();
        let mut t = 0.25;
        while t <= cycle {
            if Self::allows_go(self.state_at(i, self.time + t)) {
                return Some(t as f32);
            }
            t += 0.25;
        }
        None
    }

    /// Index of the phase light `i` is in (debugging).
    pub fn phase_index(&self, i: usize) -> i32 {
        let phases = match self.lights.get(i) {
            Some(p) if !p.is_empty() => p,
            _ => return -1,
        };
        let mut x = self.time.rem_euclid(self.cycle_len()) as f32;
        for (k, (_, dur)) in phases.iter().enumerate() {
            if x < *dur {
                return k as i32;
            }
            x -= dur;
        }
        phases.len() as i32 - 1
    }

    /// Seconds until light `i` leaves the state it shows now while the clock runs on
    /// (phases in a row with the same state count as one; a stop point may hold it
    /// longer). A pedestrian starts across only when the green lasts.
    pub fn remaining(&self, i: usize) -> f32 {
        let phases = match self.lights.get(i) {
            Some(p) if !p.is_empty() => p,
            _ => return f32::INFINITY,
        };
        let cycle = self.cycle_len() as f32;
        let x = self.time.rem_euclid(self.cycle_len()) as f32;
        // the schedule of one cycle: (state, start, end); a last phase of length 0 (or a
        // cycle longer than the phases) lasts until the cycle ends
        let mut spans: Vec<(i32, f32, f32)> = Vec::new();
        let mut start = 0.0;
        for (k, (state, dur)) in phases.iter().enumerate() {
            let end = if k + 1 == phases.len() {
                cycle.max(start + dur)
            } else {
                start + dur
            };
            spans.push((*state, start, end));
            start = end;
        }
        let Some(k) = spans.iter().position(|s| x < s.2).or(Some(spans.len() - 1)) else {
            return f32::INFINITY;
        };
        let state = spans[k].0;
        if spans.iter().all(|s| s.0 == state) {
            return f32::INFINITY;
        }
        // run on through the following phases (wrapping round) while the state stays
        let mut left = spans[k].2 - x;
        let mut j = (k + 1) % spans.len();
        while spans[j].0 == state && j != k {
            left += spans[j].2 - spans[j].1;
            j = (j + 1) % spans.len();
        }
        left
    }

    /// OMSI light states as the stock lamp scripts read them: 0..2 red, 3..5 red and
    /// yellow, 6..8 green (8: the GDR's green with yellow), 9..11 yellow, 12 and above dark.
    /// (The code once read 8 as yellow and 9 as all-red: every stock program's yellow, 9,
    /// showed red, so the lights went from green straight to red.)
    pub fn aspect(state: i32) -> Aspect {
        match state {
            0..=2 => Aspect::Red,
            3..=5 => Aspect::RedYellow,
            6 | 7 => Aspect::Green,
            8 => Aspect::GreenYellow,
            9..=11 => Aspect::Yellow,
            _ => Aspect::Dark,
        }
    }

    /// What light `i` tells a vehicle now. A light that shows green in its cycle and is dark
    /// while the program runs is not switched off but not this movement's turn: a turn
    /// arrow (`KOR` of BRT Berlin's crossings is dark for 34 s of its cycle while the cross
    /// traffic has green) holds its traffic like red. A light that never shows green (a
    /// level crossing's, dark until a train comes) and a program that is switched off or
    /// only flashes yellow stay dark, and the right of way applies.
    pub fn vehicle_aspect(&self, i: usize) -> Aspect {
        match Self::aspect(self.state(i)) {
            Aspect::Dark if self.has_green(i) && self.running() => Aspect::Red,
            a => a,
        }
    }

    /// Does light `i` show green anywhere in its cycle?
    fn has_green(&self, i: usize) -> bool {
        self.lights.get(i).is_some_and(|p| {
            p.iter().any(|&(s, d)| d > 0.0 && matches!(Self::aspect(s), Aspect::Green | Aspect::GreenYellow))
        })
    }

    /// Does the program run its signals now: some light shows red or green (not every light
    /// dark or flashing yellow, as a program switched off for the night)?
    pub fn running(&self) -> bool {
        (0..self.lights.len()).any(|k| {
            !matches!(Self::aspect(self.state(k)), Aspect::Dark | Aspect::Yellow)
        })
    }

    /// May a vehicle drive over the stop line now (not counting yellow, which is the
    /// driver's decision)?
    pub fn allows_go(state: i32) -> bool {
        matches!(Self::aspect(state), Aspect::Green | Aspect::Dark)
    }

    /// Lamp variables (red, yellow, green) of a state, for lamp objects without a script
    /// (the rules of the stock `ampel1_ddr.osc`, which agree with `ampel1.osc` for the
    /// states the West Berlin programs use).
    pub fn lamps(state: i32) -> (bool, bool, bool) {
        (
            matches!(state, 0..=5),
            matches!(state, 3..=5 | 8..=11),
            matches!(state, 6..=8),
        )
    }
}

/// What a traffic light shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aspect {
    Red,
    RedYellow,
    Green,
    GreenYellow,
    Yellow,
    Dark,
}

#[cfg(test)]
mod light_tests {
    use super::TrafficLightController;

    #[test]
    fn remaining_green_of_a_pedestrian_light() {
        let at = |mut c: TrafficLightController, t: f64| {
            c.time = t;
            c
        };
        // Einm_Stresow_Obermeier "Main_Ped": green 8 s, then red to the end of a 38 s cycle
        let ped = TrafficLightController::new(vec![vec![(6, 8.0), (0, 0.0)]], 38.0);
        let c = at(ped.clone(), 3.0);
        assert_eq!(c.state(0), 6);
        assert!((c.remaining(0) - 5.0).abs() < 1e-4);
        let c = at(ped, 20.0);
        assert_eq!(c.state(0), 0);
        assert!((c.remaining(0) - 18.0).abs() < 1e-4);
        // red running over the end of the cycle into a red start
        let d = at(
            TrafficLightController::new(
                vec![vec![(0, 19.0), (3, 2.0), (6, 11.0), (9, 3.0), (0, 0.0)]],
                38.0,
            ),
            36.0,
        );
        assert!((d.remaining(0) - 21.0).abs() < 1e-4, "{}", d.remaining(0));
        assert!(
            at(TrafficLightController::new(vec![vec![(6, 5.0)]], 0.0), 1.0)
                .remaining(0)
                .is_infinite()
        );
    }
}
