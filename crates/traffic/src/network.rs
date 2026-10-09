use glam::{DVec2, DVec3};
use hashbrown::HashMap;
use crate::ids::NetworkVersion;
use crate::rules::{DEFAULT_PRIORITY, pool_density};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneKind {
    Street,
    Sidewalk,
    Rail,
    /// `[path]` type 3: flight paths of AI aircraft.
    Air,
}

impl LaneKind {
    pub fn from_code(c: i32) -> LaneKind {
        match c {
            1 => LaneKind::Sidewalk,
            2 => LaneKind::Rail,
            3 => LaneKind::Air,
            _ => LaneKind::Street,
        }
    }
}

/// Identity of a lane in map terms: the tile, the spline/object id and the `[path]` index.
/// Timetable tracks reference lanes this way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LaneKey {
    pub tile: (i32, i32),
    pub id: i64,
    pub path: u16,
}

/// One `[blockpath] <path> <mode>` entry: another `[path]` of the same object this lane
/// blocks while it is taken. The mode's full meaning is not established; it is kept as
/// content data (with provenance at the adapter) rather than discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRule {
    /// The other `[path]` index of the same object.
    pub path: u16,
    /// The second `[blockpath]` value, semantics unresolved.
    pub mode: u16,
}

/// A sampled lane: points with headings, in world coordinates.
#[derive(Debug, Clone)]
pub struct Lane {
    pub key: Option<LaneKey>,
    /// True for the backwards lane of a two-way path.
    pub reversed: bool,
    pub kind: LaneKind,
    pub width: f32,
    /// World positions along the lane (in travel direction).
    pub points: Vec<DVec3>,
    /// Heading (degrees, clockwise from north) at each point: the tangent of the spline or
    /// arc the lane was sampled from, not the direction of the chord to the next point.
    pub headings: Vec<f32>,
    /// Signed curvature at each point (1/m, positive = turning right).
    pub curvature: Vec<f32>,
    /// Cumulative distance at each point.
    pub dist: Vec<f32>,
    pub speed_limit_kmh: f32,
    /// Lanes reachable from the end of this one.
    pub next: Vec<usize>,
    /// Traffic light controlling entry into this lane: (object instance, light index).
    pub traffic_light: Option<(usize, usize)>,
    /// Turn indicator for AI: 0 none, 1 left, 2 right.
    pub turn: i32,
    /// Source for debugging.
    pub source: u32,
    /// Lateral offset of the spline path (m, positive = right of the spline direction).
    pub offset: f32,
    /// Spline/object file the lane came from (debugging).
    pub name: String,
    /// `[rule] trafficdensity`: how much of the road traffic uses this lane (0 = none), of
    /// any random traffic group.
    pub density: f32,
    /// `[rule] trafficdensity <value> <group>` of the path, the last per group: the group
    /// is the random traffic group's place in the map's `unsched_vehgroups.txt`. A group
    /// without one takes its default there (Berlin-Spandau's GDR cars only drive where
    /// the Falkensee paths ask for them).
    pub group_density: Vec<(u16, f32)>,
    /// `[rule] no_cars`: cars keep off this lane.
    pub no_cars: bool,
    /// `[rule] bus` / `[rule] trucks` on the path: it is open to the AI vehicles of
    /// `[ai_veh_type]` 2 / 3 (see [`Lane::allows`]). The rules are switches: the value
    /// after them is not read (Grundorf's 110 `trucks` rules all say 0).
    pub rule_bus: bool,
    pub rule_trucks: bool,
    /// The spline this lane belongs to is editor-only: OMSI's invisible service roads at
    /// the edge of a map, where AI traffic drives with no road drawn under it.
    pub invisible: bool,
    /// Parallel lanes of the same road in the same direction (for lane changes).
    pub left: Option<usize>,
    pub right: Option<usize>,
    /// `[rule] priority` (`TPRIPriority`): who goes first where two paths of a junction
    /// meet. The stock maps put 192 on the straight paths of the main road and 64 on the
    /// paths coming out of a side road and leave the rest alone, so an unmarked path sits
    /// in between at `DEFAULT_PRIORITY`.
    pub priority: f32,
    /// Other `[path]`s of its object this lane blocks while taken (`[blockpath]`), beyond
    /// the places where they cross.
    pub blocks: Vec<BlockRule>,
    /// `[crossingproblem]` after this `[path]`: a vehicle on it keeps the junction clear.
    /// Its exact decision semantics are not established; it is carried as content data.
    pub crossing_problem: bool,
}


impl Lane {
    /// May an AI vehicle of `[ai_veh_type]` `veh_type` drive here (Omsi.exe 0x71d714, by
    /// the path's rules): a car (0) where there is no `no_cars`, a taxi (1) also where
    /// `bus` or `trucks` opens a no_cars path, a bus (2) only where `bus` and a truck (3)
    /// only where `trucks` is set. Other values (and the timetable's buses, -1) anywhere.
    pub fn allows(&self, veh_type: i32) -> bool {
        match veh_type {
            0 => !self.no_cars,
            1 => !self.no_cars || self.rule_bus || self.rule_trucks,
            2 => self.rule_bus,
            3 => self.rule_trucks,
            _ => true,
        }
    }

    /// How much of `unsched_vehgroups.txt` group `pool`'s traffic the lane carries (see
    /// [`pool_density`]).
    pub fn pool_density(&self, defaults: &[i32], pool: usize) -> f32 {
        pool_density(&self.group_density, defaults, pool)
    }

    /// Closest point of the lane's polyline to `p`: (distance along the lane, distance to it).
    pub fn nearest_point(&self, p: DVec3) -> Option<(f32, f64)> {
        let mut best: Option<(f32, f64)> = None;
        for k in 0..self.points.len().saturating_sub(1) {
            let a = self.points[k];
            let b = self.points[k + 1];
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
            let d = (a + ab * t - p).length();
            if best.map(|b| d < b.1).unwrap_or(true) {
                best = Some((
                    self.dist[k] + (self.dist[k + 1] - self.dist[k]) * t as f32,
                    d,
                ));
            }
        }
        best
    }

    pub fn length(&self) -> f32 {
        *self.dist.last().unwrap_or(&0.0)
    }

    /// Measure the lane again after its points were moved (an object's tilt).
    pub fn refresh(&mut self) {
        self.dist = cumulative(&self.points);
    }

    /// Segment and fraction along it at distance `s` (clamped to the lane).
    fn locate(&self, s: f32) -> (usize, f32) {
        // (a position gone NaN takes the lane's start rather than stopping the game)
        let s = if s.is_nan() {
            0.0
        } else {
            s.clamp(0.0, self.length())
        };
        let i = match self.dist.binary_search_by(|d| d.total_cmp(&s)) {
            Ok(i) => i.min(self.points.len() - 2),
            Err(i) => i.saturating_sub(1).min(self.points.len() - 2),
        };
        let (d0, d1) = (self.dist[i], self.dist[i + 1]);
        (i, if d1 > d0 { (s - d0) / (d1 - d0) } else { 0.0 })
    }

    /// Position and heading at distance `s`. Between two samples the lane is a cubic
    /// Hermite curve whose tangents are the sampled headings, so a bend stays round
    /// instead of becoming a chain of chords: a car following the chords turned in small
    /// jerks at every sample and at every lane joint.
    pub fn at(&self, s: f32) -> (DVec3, f32) {
        if self.points.len() < 2 {
            return (
                self.points.first().copied().unwrap_or(DVec3::ZERO),
                self.headings.first().copied().unwrap_or(0.0),
            );
        }
        let (i, t) = self.locate(s);
        let (h0, h1) = (self.headings[i], self.headings[i + 1]);
        (self.hermite(i, t as f64), h0 + wrap_deg(h1 - h0) * t)
    }

    fn hermite(&self, i: usize, t: f64) -> DVec3 {
        let (a, b) = (self.points[i], self.points[i + 1]);
        let lin = a.lerp(b, t);
        let chord = (b - a).truncate();
        let len = chord.length();
        if len < 1e-6 {
            return lin;
        }
        let d0 = heading_dir(self.headings[i] as f64);
        let d1 = heading_dir(self.headings[i + 1] as f64);
        // tangents far off the chord (a kink in hand-made samples) would make the curve loop
        let c = chord / len;
        if d0.dot(c) < 0.8 || d1.dot(c) < 0.8 {
            return lin;
        }
        let (t2, t3) = (t * t, t * t * t);
        let xy = a.truncate() * (2.0 * t3 - 3.0 * t2 + 1.0)
            + d0 * len * (t3 - 2.0 * t2 + t)
            + b.truncate() * (3.0 * t2 - 2.0 * t3)
            + d1 * len * (t3 - t2);
        DVec3::new(xy.x, xy.y, lin.z)
    }

    /// Position and heading at `s`, continued straight along the end tangents beyond either
    /// end of the lane.
    pub fn at_ext(&self, s: f32) -> (DVec3, f32) {
        if s < 0.0 {
            let h = self.start_heading();
            let d = heading_dir(h as f64) * s as f64;
            (self.start() + DVec3::new(d.x, d.y, 0.0), h)
        } else if s > self.length() {
            let h = self.end_heading();
            let d = heading_dir(h as f64) * (s - self.length()) as f64;
            (self.end() + DVec3::new(d.x, d.y, 0.0), h)
        } else {
            self.at(s)
        }
    }

    /// Curvature at `s` (1/m, positive = right).
    pub fn curvature_at(&self, s: f32) -> f32 {
        if self.points.len() < 2 || self.curvature.len() != self.points.len() {
            return 0.0;
        }
        let (i, t) = self.locate(s);
        self.curvature[i] + (self.curvature[i + 1] - self.curvature[i]) * t
    }

    pub fn start(&self) -> DVec3 {
        self.points[0]
    }
    pub fn end(&self) -> DVec3 {
        *self.points.last().unwrap()
    }
    pub fn start_heading(&self) -> f32 {
        self.headings[0]
    }
    pub fn end_heading(&self) -> f32 {
        *self.headings.last().unwrap()
    }
}

/// Speed limit of a flight path without a `[rule] speedlimit` (km/h): none, the aircraft
/// flies at its own speed.
pub const AIR_NO_LIMIT_KMH: f32 = 1000.0;

/// Builds lanes from analytic descriptions.
pub struct LaneBuilder;

impl LaneBuilder {
    /// Sample an arc/straight lane: start position, heading (deg), length, radius (0 = straight,
    /// > 0 right turn), height change over the length.
    pub fn arc(
        start: DVec3,
        heading_deg: f64,
        length: f64,
        radius: f64,
        dz: f64,
        kind: LaneKind,
        width: f32,
    ) -> Lane {
        let n = ((length / 2.0).ceil() as usize).clamp(1, 400);
        let mut points = Vec::with_capacity(n + 1);
        let mut headings = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let s = length * i as f64 / n as f64;
            let (p, h) = arc_point(start, heading_deg, s, radius);
            points.push(DVec3::new(p.x, p.y, start.z + dz * s / length.max(1e-6)));
            headings.push(h as f32);
        }
        let k = if radius.abs() < 1e-6 {
            0.0
        } else {
            (1.0 / radius) as f32
        };
        Self::curve(points, headings, vec![k; n + 1], kind, width)
    }

    /// Lane from sampled points with their tangent headings and curvatures.
    /// A flight path has no speed limit of its own unless the map gives it one with a
    /// `[rule] speedlimit`; the street default of 50 km/h had the Tegel approach flown at
    /// walking pace for an airliner.
    pub fn curve(
        points: Vec<DVec3>,
        headings: Vec<f32>,
        curvature: Vec<f32>,
        kind: LaneKind,
        width: f32,
    ) -> Lane {
        let dist = cumulative(&points);
        let speed_limit_kmh = if kind == LaneKind::Air {
            AIR_NO_LIMIT_KMH
        } else {
            50.0
        };
        Lane {
            key: None,
            reversed: false,
            kind,
            width,
            points,
            headings,
            curvature,
            dist,
            speed_limit_kmh,
            next: Vec::new(),
            traffic_light: None,
            turn: 0,
            source: 0,
            offset: 0.0,
            name: String::new(),
            invisible: false,
            density: 1.0,
            group_density: Vec::new(),
            no_cars: false,
            rule_bus: false,
            rule_trucks: false,
            left: None,
            right: None,
            priority: DEFAULT_PRIORITY,
            blocks: Vec::new(),
            crossing_problem: false,
        }
    }

    /// Lane from bare points: headings from the neighbouring points on both sides (the
    /// tangent there, not the chord ahead), curvature from how the heading changes.
    pub fn polyline(points: Vec<DVec3>, kind: LaneKind, width: f32) -> Lane {
        let n = points.len();
        let mut headings = Vec::with_capacity(n);
        for i in 0..n {
            let (a, b) = match (i.checked_sub(1), (i + 1 < n).then_some(i + 1)) {
                (Some(p), Some(q)) => (points[p], points[q]),
                (None, Some(q)) => (points[i], points[q]),
                (Some(p), None) => (points[p], points[i]),
                (None, None) => (points[i], points[i] + DVec3::Y),
            };
            let d = b - a;
            headings.push((d.x.atan2(d.y)).to_degrees() as f32);
        }
        let dist = cumulative(&points);
        let curvature = (0..n)
            .map(|i| {
                let (p, q) = (i.saturating_sub(1), (i + 1).min(n.saturating_sub(1)));
                let ds = dist.get(q).copied().unwrap_or(0.0) - dist.get(p).copied().unwrap_or(0.0);
                if ds > 1e-3 {
                    wrap_deg(headings[q] - headings[p]).to_radians() / ds
                } else {
                    0.0
                }
            })
            .collect();
        Self::curve(points, headings, curvature, kind, width)
    }
}

/// Cumulative distance along a sequence of points.
fn cumulative(points: &[DVec3]) -> Vec<f32> {
    let mut acc = 0.0f32;
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i > 0 {
                acc += (*p - points[i - 1]).length() as f32;
            }
            acc
        })
        .collect()
}

/// An angle difference in degrees brought into -180..180.
pub fn wrap_deg(d: f32) -> f32 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

/// Unit vector (east, north) of a heading in degrees.
fn heading_dir(h: f64) -> DVec2 {
    let r = h.to_radians();
    DVec2::new(r.sin(), r.cos())
}

/// Point and heading after travelling `s` metres on an arc starting at `start`.
pub fn arc_point(start: DVec3, heading_deg: f64, s: f64, radius: f64) -> (DVec2, f64) {
    let h = heading_deg.to_radians();
    let d = DVec2::new(h.sin(), h.cos());
    if radius.abs() < 1e-6 {
        return (start.truncate() + d * s, heading_deg);
    }
    let r = radius.abs();
    let turn = radius.signum();
    let right = DVec2::new(d.y, -d.x);
    let centre = start.truncate() + right * r * turn;
    let ang = -turn * s / r;
    let p = start.truncate() - centre;
    let (sa, ca) = ang.sin_cos();
    (
        centre + DVec2::new(p.x * ca - p.y * sa, p.x * sa + p.y * ca),
        heading_deg + turn * (s / r).to_degrees(),
    )
}

/// The whole network.
#[derive(Default)]
pub struct Network {
    pub lanes: Vec<Lane>,
    /// Lanes by map identity (both directions of a two-way path share a key).
    pub by_key: HashMap<LaneKey, Vec<usize>>,
    /// Per lane: lanes of the same crossing object whose geometry crosses or merges into it.
    pub conflicts: Vec<Vec<usize>>,
    /// Per lane: the lanes that lead into it (the reverse of `next`), for walking a
    /// one-way path backwards.
    pub prev: Vec<Vec<usize>>,
    /// Lanes by 50 m grid cell (every cell a lane's points touch), so that a nearest-lane
    /// query looks at a handful of lanes instead of all 17 000 of Spandau.
    pub grid: HashMap<(i32, i32), Vec<usize>>,
    /// Lanes by the cell containing their start. Population only needs lane starts near
    /// a viewer; the geometry grid above can contain the same long lane in many cells.
    pub start_grid: HashMap<(i32, i32), Vec<usize>>,
    /// Per street lane: where the other street and rail lanes of its junction object cross
    /// it or run into its end (`conflicts` with the places).
    pub crossings: Vec<Vec<Crossing>>,
    /// Per street lane: the footpaths of its junction object that cross it (zebra and
    /// signalled crossings): (footpath lane, distance along the street lane, along the path).
    pub walks: Vec<Vec<(usize, f32, f32)>>,
    /// Per lane: how far a vehicle can drive from its start before the network ends (m, at
    /// most `REACH_MAX`; lanes closed to cars do not count as a way on).
    pub reach: Vec<f32>,
    /// Per lane: it runs round a roundabout (see `compute_rings`).
    pub ring: Vec<bool>,
    /// The map drives on the left (`global.cfg` `[lht]`): priority to the left, the
    /// oncoming lane on the right, turning right across the oncoming traffic.
    pub left_hand: bool,
    /// Bumped whenever lanes or links change, so callers can invalidate cached route and
    /// conflict work. Starts at a nonzero value so a default-built network is versioned.
    pub version: NetworkVersion,
}

impl Network {
    /// The current network version (changes when lanes or their links change).
    pub fn version(&self) -> NetworkVersion {
        let mut v = self.version;
        if v.get() == 0 {
            v = NetworkVersion(1);
        }
        v
    }

    /// Mark the network changed (a streamed tile added lanes or links).
    pub fn bump_version(&mut self) {
        self.version = NetworkVersion(self.version().get().wrapping_add(1).max(1));
    }
}

/// `Network::reach` is counted up to this far (m).
pub const REACH_MAX: f32 = 600.0;
/// A way on that ends within this distance is a dead end to a driver who has a choice (m).
pub const DEAD_END: f32 = 500.0;
/// The longest roundabout (m round) and the longest lane of one that `compute_rings` finds:
/// a 60 m wide circle is 190 m round; a block of streets is longer.
pub const RING_MAX: f32 = 200.0;
pub const RING_LANE_MAX: f32 = 60.0;

/// Where two lanes of a junction meet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    pub other: usize,
    /// Distance along this lane and along the other one to the meeting point.
    pub at: f32,
    pub other_at: f32,
    /// Both lanes end in the same place (one road runs into the other) rather than crossing.
    pub merge: bool,
    /// The meeting place: how far before and after `at` this lane's centre line stays
    /// closer than `MEET_DIST` to the other's (where two vehicles on them would touch),
    /// and the same along the other lane. Two paths that cross at a shallow angle, or two
    /// turns that bend towards each other, meet over metres, not at a point.
    pub before: f32,
    pub after: f32,
    pub other_before: f32,
    pub other_after: f32,
}

/// Two vehicles whose centre lines come closer than this (m) touch: the half widths of two
/// buses.
pub const MEET_DIST: f64 = 2.6;
/// A meeting place reaches at most this far either side of the crossing point (m).
const MEET_MAX: f32 = 14.0;
/// Two lanes whose centre lines cross in plan but whose bodies are this far apart
/// vertically (m) do not meet: a bridge does not conflict with the road under it.
pub const MEET_CLEARANCE: f64 = 4.0;

/// How far before and after distance `at` of lane `a` its centre line stays within
/// `MEET_DIST` of lane `b`'s (in half-metre steps, at least the half metre around the point).
fn meeting_extent(a: &Lane, at: f32, b: &Lane) -> (f32, f32) {
    let near = |s: f32| {
        b.nearest_point(a.at(s).0)
            .map(|(_, d)| d < MEET_DIST)
            .unwrap_or(false)
    };
    let mut before = 0.5f32;
    while before < MEET_MAX && at - before >= 0.0 && near(at - before) {
        before += 0.5;
    }
    let mut after = 0.5f32;
    while after < MEET_MAX && at + after <= a.length() && near(at + after) {
        after += 0.5;
    }
    (before, after)
}

/// Where two polylines cross: the distance along each (ends excluded).
fn polyline_crossing(a: &Lane, b: &Lane) -> Option<(f32, f32)> {
    for (i, pa) in a.points.windows(2).enumerate() {
        for (j, pb) in b.points.windows(2).enumerate() {
            let (p, r) = (pa[0].truncate(), (pa[1] - pa[0]).truncate());
            let (q, s) = (pb[0].truncate(), (pb[1] - pb[0]).truncate());
            let den = r.perp_dot(s);
            if den.abs() < 1e-9 {
                continue;
            }
            let t = (q - p).perp_dot(s) / den;
            let u = (q - p).perp_dot(r) / den;
            if !(0.0..=1.0).contains(&t) || !(0.0..=1.0).contains(&u) {
                continue;
            }
            let sa = a.dist[i] + (a.dist[i + 1] - a.dist[i]) * t as f32;
            let sb = b.dist[j] + (b.dist[j + 1] - b.dist[j]) * u as f32;
            // meeting at an end is a joint (or a fork), not a crossing
            let inner = |s: f32, l: &Lane| s > 0.3 && s < l.length() - 0.3;
            if inner(sa, a) && inner(sb, b) {
                return Some((sa, sb));
            }
        }
    }
    None
}

/// Grid cell size of `Network::grid` (m).
pub const GRID_CELL: f64 = 50.0;

impl Network {
    /// The sign of a lateral offset (positive to the right) towards the oncoming lane:
    /// -1 on the right-hand side of the road, +1 on a left-hand-traffic map.
    pub fn oncoming_sign(&self) -> f32 {
        if self.left_hand { 1.0 } else { -1.0 }
    }

    /// Lane with the given key, preferring the direction flag.
    pub fn find(&self, key: LaneKey, reversed: Option<bool>) -> Option<usize> {
        let list = self.by_key.get(&key)?;
        match reversed {
            Some(r) => list
                .iter()
                .copied()
                .find(|&i| self.lanes[i].reversed == r)
                .or_else(|| list.first().copied()),
            None => list.first().copied(),
        }
    }

    /// Does `b` run beside `a` from about where `a` starts - another lane of the same spline,
    /// or the other branch of a fork in the same junction - without `a` leading into it? A
    /// timetable track that lists two such paths one after the other changes lanes there
    /// (Spandau's line 92 moves from lane 5 to lane 6 of a six-lane Falkenseer Chaussee
    /// piece that way); driving `a` to its end first and then starting `b` sent the bus
    /// 30 m back and sideways.
    ///
    /// Or `b` runs beside a stretch of `a` without starting where it does: a bus lane or
    /// stop lane of a junction object that begins before or after the lane the route came
    /// in on (Spandau's Klosterstraße, Heerstraße and Altstädter Ring junctions: the station
    /// link reaches the stop on the through lane, the next one leaves from the stop lane
    /// beside it). Driven one after the other the bus jumped up to 58 m back.
    pub fn parallel(&self, a: usize, b: usize) -> bool {
        let (Some(la), Some(lb)) = (self.lanes.get(a), self.lanes.get(b)) else {
            return false;
        };
        if a == b || la.next.contains(&b) {
            return false;
        }
        let same = match (la.key, lb.key) {
            (Some(x), Some(y)) => x.tile == y.tile && x.id == y.id && x.path != y.path,
            _ => false,
        };
        if same
            && (lb.start() - la.start()).truncate().length() < 8.0
            && (lb.start().z - la.start().z).abs() < 1.0
            && wrap_deg(lb.start_heading() - la.start_heading()).abs() < 45.0
        {
            return true;
        }
        // (a lane of another spline beside it counts as well: mod maps lay a bus lane or a
        // second carriageway as splines of their own, and Novi Sad's tracks change onto
        // them 30 m before the lane they come in on ends)
        self.beside(la, lb)
    }

    /// Both lanes continue as the same adjacent pair, without a fork or a sharp joint.
    /// A spline boundary on such a road is not the end of a lane-change corridor.
    pub fn parallel_continuation(&self, a: usize, b: usize) -> Option<(usize, usize)> {
        let (la, lb) = (self.lanes.get(a)?, self.lanes.get(b)?);
        let ([na], [nb]) = (la.next.as_slice(), lb.next.as_slice()) else { return None };
        let (next_a, next_b) = (self.lanes.get(*na)?, self.lanes.get(*nb)?);
        let paired = (la.left == Some(b) && next_a.left == Some(*nb))
            || (la.right == Some(b) && next_a.right == Some(*nb));
        if !paired { return None; }
        for (from, to) in [(la, next_a), (lb, next_b)] {
            if from.kind != to.kind || (from.end() - to.start()).length() > 1.5
                || wrap_deg(from.end_heading() - to.start_heading()).abs() > 10.0
            { return None; }
        }
        Some((*na, *nb))
    }

    /// Does lane `b` run beside `a` (a lane's width or so to the side, the same way) along
    /// at least 8 m of it?
    fn beside(&self, la: &Lane, lb: &Lane) -> bool {
        let n = 8;
        let (mut along, step) = (0.0f32, la.length() / n as f32);
        for i in 0..=n {
            let s = la.length() * i as f32 / n as f32;
            let (p, h) = la.at(s);
            let Some((sb, d)) = lb.nearest_point(p) else {
                continue;
            };
            let inside = sb > 0.5 && sb < lb.length() - 0.5;
            // (on the same level: a track or road under a bridge lies a few metres off in 3D
            // too, and a train "changed lanes" down onto the line under its viaduct)
            let (q, hb) = lb.at(sb);
            let level = (q.z - p.z).abs() < 1.0;
            if inside && level && d > 0.8 && d < 5.5 && wrap_deg(hb - h).abs() < 30.0 {
                along += step;
            }
        }
        along >= 8.0 - 1e-3
    }

    /// Where on lane `b`, beside `a` (see `parallel`), the car at distance `s` along `a` is:
    /// the same distance on lanes that start together, else the point of `b` beside it.
    pub fn beside_s(&self, a: usize, b: usize, s: f32) -> f32 {
        let (la, lb) = (&self.lanes[a], &self.lanes[b]);
        if (lb.start() - la.start()).truncate().length() < 8.0 {
            let frac = if la.length() > 0.0 {
                s / la.length()
            } else {
                0.0
            };
            return frac * lb.length();
        }
        (s + self.beside_delta(a, b)).clamp(0.0, lb.length())
    }

    /// How much further along `b` than along `a` a point beside both lies (see `parallel`):
    /// 0 for lanes that start together, the distance `b` starts before `a` (positive) or
    /// after it (negative) otherwise.
    pub fn beside_delta(&self, a: usize, b: usize) -> f32 {
        let (la, lb) = (&self.lanes[a], &self.lanes[b]);
        if (lb.start() - la.start()).truncate().length() < 8.0 {
            return 0.0;
        }
        match la.nearest_point(lb.start()) {
            Some((sa, _)) if sa > 0.5 => -sa,
            _ => lb
                .nearest_point(la.start())
                .map(|(sb, _)| sb)
                .unwrap_or(0.0),
        }
    }

    /// Point on a lane sequence closest to `p`: (index into `route`, distance along that lane).
    pub fn project_on_route(&self, route: &[usize], p: DVec3) -> Option<(usize, f32)> {
        self.project_on_route_lateral(route, p)
            .map(|(ri, s, _)| (ri, s))
    }

    /// Like `project_on_route`, plus the lateral offset of `p` from the lane (m, right of
    /// the driving direction positive).
    pub fn project_on_route_lateral(&self, route: &[usize], p: DVec3) -> Option<(usize, f32, f32)> {
        let mut best: Option<(usize, f32, f64, f32)> = None;
        for (ri, &li) in route.iter().enumerate() {
            let l = &self.lanes[li];
            for k in 0..l.points.len().saturating_sub(1) {
                let a = l.points[k];
                let b = l.points[k + 1];
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
                let q = a + ab * t;
                let d = (q - p).truncate().length();
                if best.map(|b| d < b.2).unwrap_or(true) {
                    let dir = ab.truncate().normalize_or_zero();
                    let rel = (p - q).truncate();
                    // right vector of (dir.x, dir.y) is (dir.y, -dir.x)
                    let lateral = (rel.x * dir.y - rel.y * dir.x) as f32;
                    best = Some((
                        ri,
                        l.dist[k] + (l.dist[k + 1] - l.dist[k]) * t as f32,
                        d,
                        lateral,
                    ));
                }
            }
        }
        best.map(|(ri, s, _, lat)| (ri, s, lat))
    }

    /// Where a bus stop at `p` lies on `route`: like [`Network::project_on_route_lateral`],
    /// but among the points within `reach` (when given) and not before route index
    /// `from`, a lane with the stop on its kerb side (right, or left with left-hand traffic)
    /// goes before a nearer one with the stop across it - a route back along the same
    /// street passes each stop twice, once from the other side, and the bus stopped at the
    /// stop across the road on its way out. Falls back to the nearest point.
    pub fn project_stop_on_route(
        &self,
        route: &[usize],
        p: DVec3,
        reach: Option<f64>,
        from: usize,
    ) -> Option<(usize, f32, f32)> {
        // (index, s, distance, lateral) of the best on the kerb side, and of any
        let mut kerb: Option<(usize, f32, f64, f32)> = None;
        let mut any: Option<(usize, f32, f64, f32)> = None;
        for (ri, &li) in route.iter().enumerate().skip(from.min(route.len())) {
            let l = &self.lanes[li];
            for k in 0..l.points.len().saturating_sub(1) {
                let a = l.points[k];
                let b = l.points[k + 1];
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
                let q = a + ab * t;
                let d = (q - p).truncate().length();
                if reach.is_some_and(|r| d > r) {
                    continue;
                }
                let dir = ab.truncate().normalize_or_zero();
                let rel = (p - q).truncate();
                let lateral = (rel.x * dir.y - rel.y * dir.x) as f32;
                let cand = (
                    ri,
                    l.dist[k] + (l.dist[k + 1] - l.dist[k]) * t as f32,
                    d,
                    lateral,
                );
                let kerb_side = if self.left_hand {
                    lateral < -0.3
                } else {
                    lateral > 0.3
                };
                if kerb_side && kerb.map(|b| d < b.2).unwrap_or(true) {
                    kerb = Some(cand);
                }
                if any.map(|b| d < b.2).unwrap_or(true) {
                    any = Some(cand);
                }
            }
        }
        match kerb.or(any) {
            Some((ri, s, _, lat)) => Some((ri, s, lat)),
            None if from > 0 => self.project_stop_on_route(route, p, reach, 0),
            None => None,
        }
    }

    /// Geometric conflicts between the lanes of each crossing object (paths that intersect
    /// or end at the same point, with where they meet), and the footpaths crossing its
    /// street lanes. AI cars sort out who goes first from these (`Network::must_yield`).
    pub fn compute_conflicts(&mut self) {
        self.conflicts = vec![Vec::new(); self.lanes.len()];
        self.crossings = vec![Vec::new(); self.lanes.len()];
        self.walks = vec![Vec::new(); self.lanes.len()];
        let (n, walks) = self.conflicts_from(0);
        log::info!(
            "path network: {n} conflicting lane pairs at crossings, {walks} footpath crossings"
        );
    }

    /// `conflicts`, `crossings` and `walks` of the junction objects whose lanes start at
    /// index `first` (all lanes of an object come with its tile, so a grown network only
    /// needs its new objects looked at). Returns the pairs and footpath crossings found.
    fn conflicts_from(&mut self, first: usize) -> (usize, usize) {
        let len = self.lanes.len();
        self.conflicts.resize(len, Vec::new());
        self.crossings.resize(len, Vec::new());
        self.walks.resize(len, Vec::new());
        let mut by_object: HashMap<((i32, i32), i64), Vec<usize>> = HashMap::new();
        let mut walks_by_object: HashMap<((i32, i32), i64), Vec<usize>> = HashMap::new();
        for (i, l) in self.lanes.iter().enumerate().skip(first) {
            if l.source != 2 {
                continue;
            }
            let Some(k) = l.key else { continue };
            match l.kind {
                // a level crossing is a junction of a road and a railway
                LaneKind::Street | LaneKind::Rail => {
                    by_object.entry((k.tile, k.id)).or_default().push(i)
                }
                LaneKind::Sidewalk => walks_by_object.entry((k.tile, k.id)).or_default().push(i),
                LaneKind::Air => {}
            }
        }
        let mut n = 0;
        for (key, lanes) in &by_object {
            for (x, &i) in lanes.iter().enumerate() {
                for &j in &lanes[x + 1..] {
                    let (a, b) = (&self.lanes[i], &self.lanes[j]);
                    let same_path = a.key.map(|k| k.path) == b.key.map(|k| k.path);
                    if same_path || (a.kind == LaneKind::Rail && b.kind == LaneKind::Rail) {
                        continue;
                    }
                    // one runs into the other: the same end, or ends a car's width apart that
                    // lead into the same lane (two turns converging on one exit)
                    let ends = (a.end() - b.end()).truncate().length();
                    let joint = ends < 1.5
                        || (ends < MEET_DIST && a.next.iter().any(|n| b.next.contains(n)));
                    let merge = joint && (a.start() - b.start()).truncate().length() > 3.0;
                    let place = if merge {
                        Some((a.length(), b.length()))
                    } else {
                        polyline_crossing(a, b)
                    };
                    // A lane above another (a bridge over the road below) does not meet it
                    // just because their plan views cross: require the bodies to share height.
                    let place = place.filter(|&(sa, sb)| {
                        (a.at(sa).0.z - b.at(sb).0.z).abs() <= MEET_CLEARANCE
                    });
                    // `[blockpath]`: the object says the two are in each other's way even
                    // where their lines do not cross - the whole of both is the meeting place
                    let (pa, pb) = (
                        a.key.map(|k| k.path).unwrap_or(u16::MAX),
                        b.key.map(|k| k.path).unwrap_or(u16::MAX),
                    );
                    let blocked = a.blocks.iter().any(|r| r.path == pb)
                        || b.blocks.iter().any(|r| r.path == pa);
                    if place.is_none() && blocked {
                        let (la, lb) = (a.length(), b.length());
                        self.conflicts[i].push(j);
                        self.conflicts[j].push(i);
                        self.crossings[i].push(Crossing {
                            other: j,
                            at: la * 0.5,
                            other_at: lb * 0.5,
                            merge: false,
                            before: la * 0.5,
                            after: la * 0.5,
                            other_before: lb * 0.5,
                            other_after: lb * 0.5,
                        });
                        self.crossings[j].push(Crossing {
                            other: i,
                            at: lb * 0.5,
                            other_at: la * 0.5,
                            merge: false,
                            before: lb * 0.5,
                            after: lb * 0.5,
                            other_before: la * 0.5,
                            other_after: la * 0.5,
                        });
                        n += 1;
                        continue;
                    }
                    if let Some((sa, sb)) = place {
                        // (after a joint the two are in one lane, and follow each other)
                        let (ab, aa) = if merge {
                            (meeting_extent(a, sa, b).0, 0.5)
                        } else {
                            meeting_extent(a, sa, b)
                        };
                        let (bb, ba) = if merge {
                            (meeting_extent(b, sb, a).0, 0.5)
                        } else {
                            meeting_extent(b, sb, a)
                        };
                        self.conflicts[i].push(j);
                        self.conflicts[j].push(i);
                        self.crossings[i].push(Crossing {
                            other: j,
                            at: sa,
                            other_at: sb,
                            merge,
                            before: ab,
                            after: aa,
                            other_before: bb,
                            other_after: ba,
                        });
                        self.crossings[j].push(Crossing {
                            other: i,
                            at: sb,
                            other_at: sa,
                            merge,
                            before: bb,
                            after: ba,
                            other_before: ab,
                            other_after: aa,
                        });
                        n += 1;
                    }
                }
            }
            if let Some(walks) = walks_by_object.get(key) {
                for &i in lanes
                    .iter()
                    .filter(|&&i| self.lanes[i].kind == LaneKind::Street)
                {
                    for &w in walks {
                        if let Some((sa, sw)) = polyline_crossing(&self.lanes[i], &self.lanes[w]) {
                            self.walks[i].push((w, sa, sw));
                        }
                    }
                }
            }
        }
        let walks: usize = self.walks[first..].iter().map(|w| w.len()).sum();
        (n, walks)
    }

    /// Must a vehicle on lane `a` let one on lane `b` go first where they meet? The path with
    /// the higher `[rule] priority` goes first; between equals, a left turn waits for the
    /// oncoming traffic, and otherwise the one coming from the right has the right of way
    /// (the German "rechts vor links" that the stock maps rely on wherever they set no
    /// priorities) - mirrored on a left-hand-traffic map. A train always goes first.
    pub fn must_yield(&self, a: usize, b: usize) -> bool {
        let (la, lb) = (&self.lanes[a], &self.lanes[b]);
        if la.kind == LaneKind::Rail {
            return false;
        }
        if lb.kind == LaneKind::Rail {
            return true;
        }
        if (la.priority - lb.priority).abs() > 0.5 {
            return la.priority < lb.priority;
        }
        // a roundabout: who enters gives way to who is on the ring (signs 205 + 215)
        if self.is_ring(a) != self.is_ring(b) {
            return self.is_ring(b);
        }
        let rel = wrap_deg(lb.start_heading() - la.start_heading());
        // (the turn across the oncoming traffic: left, or right when driving on the left)
        let across = if self.left_hand { 2 } else { 1 };
        if rel.abs() > 135.0 {
            // oncoming
            return la.turn == across && lb.turn != across;
        }
        // `b` comes from the right when it travels to the left of `a`'s way (from the left,
        // travelling to its right, on a left-hand-traffic map)
        let rel = if self.left_hand { -rel } else { rel };
        if rel < -30.0 {
            return true;
        }
        if rel > 30.0 {
            return false;
        }
        // side by side from the same direction: the one turning across the other waits
        la.turn != 0 && lb.turn == 0
    }

    /// The lane beside `lane` at `s` that carries the traffic the other way (the other half
    /// of a two-way street): (lane, distance along it at the same place, how far its middle
    /// lies over to the oncoming side - the left, or the right on a left-hand-traffic map;
    /// `oncoming_sign` turns it into a lateral offset).
    pub fn opposite(&self, lane: usize, s: f32) -> Option<(usize, f32, f32)> {
        let l = self.lanes.get(lane)?;
        let (p, h) = l.at(s);
        let hr = (h as f64).to_radians();
        let left = DVec3::new(-hr.cos(), hr.sin(), 0.0);
        let left = if self.left_hand { -left } else { left };
        let cx = (p.x / GRID_CELL).floor() as i32;
        let cy = (p.y / GRID_CELL).floor() as i32;
        let mut best: Option<(usize, f32, f32)> = None;
        let mut seen: Vec<usize> = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &i in self
                    .grid
                    .get(&(cx + dx, cy + dy))
                    .map(|v| v.as_slice())
                    .unwrap_or(&[])
                {
                    if i == lane || seen.contains(&i) {
                        continue;
                    }
                    seen.push(i);
                    let o = &self.lanes[i];
                    if o.kind != LaneKind::Street || o.no_cars {
                        continue;
                    }
                    let Some((os, d)) = o.nearest_point(p) else {
                        continue;
                    };
                    if d > 7.0 || d < 1.5 {
                        continue;
                    }
                    let (q, oh) = o.at(os);
                    let side = (q - p).dot(left) as f32;
                    if side < 1.5 || wrap_deg(oh - h).abs() < 150.0 {
                        continue;
                    }
                    if best.map(|b| side < b.2).unwrap_or(true) {
                        best = Some((i, os, side));
                    }
                }
            }
        }
        best
    }

    /// The lanes traffic comes from towards distance `s` of `lane`, walked backwards over
    /// joints and through junctions for up to `within` metres: (lane, offset, the lane it
    /// leads into on the way to `lane`). A vehicle at distance `x` along such a lane is at
    /// `x + offset` in `lane`'s own distances (negative before its start); the first entry
    /// is `lane` itself with offset 0. Where several ways lead to one lane, the shortest
    /// counts. At most `max` lanes.
    pub fn upstream(
        &self,
        lane: usize,
        s: f32,
        within: f32,
        max: usize,
    ) -> Vec<(usize, f32, Option<usize>)> {
        let mut out: Vec<(usize, f32, Option<usize>)> = vec![(lane, 0.0, None)];
        if lane >= self.lanes.len() {
            return out;
        }
        // nearest first (a junction's lanes can be reached two ways)
        let mut done = vec![false];
        while let Some(k) = (0..out.len())
            .filter(|&k| !done[k])
            .max_by(|&a, &b| out[a].1.total_cmp(&out[b].1))
        {
            done[k] = true;
            let (l, off, _) = out[k];
            // this lane starts `s - off` metres before the place: nothing before it is in reach
            if s - off > within {
                continue;
            }
            for &p in self.prev.get(l).map(|v| v.as_slice()).unwrap_or(&[]) {
                if p == lane || self.lanes[p].kind != self.lanes[lane].kind {
                    continue;
                }
                let o = off - self.lanes[p].length();
                match out.iter().position(|e| e.0 == p) {
                    Some(e) => {
                        if o > out[e].1 && !done[e] {
                            out[e] = (p, o, Some(l));
                        }
                    }
                    None => {
                        if out.len() < max {
                            out.push((p, o, Some(l)));
                            done.push(false);
                        }
                    }
                }
            }
        }
        out
    }

    /// Connect lane ends to lane starts that lie within `tol` metres with a compatible heading.
    pub fn link(&mut self, tol: f64) {
        self.bump_version();
        self.by_key.clear();
        for (i, l) in self.lanes.iter().enumerate() {
            if let Some(k) = l.key {
                self.by_key.entry(k).or_default().push(i);
            }
        }
        self.build_grid();
        // neighbouring lanes: paths of one spline running the same way, next to each other
        let mut by_spline: HashMap<((i32, i32), i64, bool), Vec<usize>> = HashMap::new();
        for (i, l) in self.lanes.iter().enumerate() {
            if let Some(k) = l.key {
                if l.source == 1 && matches!(l.kind, LaneKind::Street) {
                    by_spline
                        .entry((k.tile, k.id as i64, l.reversed))
                        .or_default()
                        .push(i);
                }
            }
        }
        let mut neighbours = 0usize;
        for (_, mut list) in by_spline {
            list.sort_by(|a, b| self.lanes[*a].offset.total_cmp(&self.lanes[*b].offset));
            for w in list.windows(2) {
                let (a, b) = (w[0], w[1]);
                let gap = self.lanes[b].offset - self.lanes[a].offset;
                if gap > 1.5 && gap < 6.0 {
                    // b lies at the larger offset: to the right when driving along the spline
                    if neighbours == 0 {
                        let p = self.lanes[a].start();
                        log::info!(
                            "first neighbouring lane pair at ({:.1}, {:.1}), heading {:.0}",
                            p.x,
                            p.y,
                            self.lanes[a].start_heading()
                        );
                    }
                    neighbours += 1;
                    let reversed = self.lanes[a].reversed;
                    if reversed {
                        self.lanes[a].left = Some(b);
                        self.lanes[b].right = Some(a);
                    } else {
                        self.lanes[a].right = Some(b);
                        self.lanes[b].left = Some(a);
                    }
                }
            }
        }
        log::info!(
            "lanes: {} neighbouring lane pairs (lane changes)",
            neighbours
        );
        let cell = 4.0;
        let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, l) in self.lanes.iter().enumerate() {
            let s = l.start();
            grid.entry(((s.x / cell).floor() as i64, (s.y / cell).floor() as i64))
                .or_default()
                .push(i);
        }
        let mut links = 0;
        for i in 0..self.lanes.len() {
            let e = self.lanes[i].end();
            let eh = self.lanes[i].end_heading();
            let kind = self.lanes[i].kind;
            let cx = (e.x / cell).floor() as i64;
            let cy = (e.y / cell).floor() as i64;
            let mut found = Vec::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(list) = grid.get(&(cx + dx, cy + dy)) {
                        for &j in list {
                            if j == i {
                                continue;
                            }
                            let l = &self.lanes[j];
                            if l.kind != kind {
                                continue;
                            }
                            let d = (l.start() - e).truncate().length();
                            let dh = wrap_deg(l.start_heading() - eh).abs();
                            if d <= tol && dh < 40.0 && (l.start().z - e.z).abs() < 3.0 {
                                found.push(j);
                            }
                        }
                    }
                }
            }
            if found.is_empty() && std::env::var_os("OMSI_DEBUG_UNLINKED").is_some() {
                // a lane end with a start of its kind near it that was not taken
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for &j in grid
                            .get(&(cx + dx, cy + dy))
                            .map(|v| v.as_slice())
                            .unwrap_or(&[])
                        {
                            let l = &self.lanes[j];
                            if j != i && l.kind == kind {
                                let d = (l.start() - e).truncate().length();
                                let dh = wrap_deg(l.start_heading() - eh).abs();
                                if d < 4.0 && dh < 40.0 {
                                    log::info!(
                                        "unlinked: lane {i} {:?} end ({:.2}, {:.2}, {:.2}) -> lane {j} {:?} start {:.2} m away, dz {:.2}, dh {:.1}",
                                        self.lanes[i].key,
                                        e.x,
                                        e.y,
                                        e.z,
                                        l.key,
                                        d,
                                        l.start().z - e.z,
                                        dh
                                    );
                                }
                            }
                        }
                    }
                }
            }
            links += found.len();
            self.lanes[i].next = found;
        }
        log::info!("path network: {} lanes, {} links", self.lanes.len(), links);
        self.prev = vec![Vec::new(); self.lanes.len()];
        for i in 0..self.lanes.len() {
            for &n in &self.lanes[i].next {
                if n < self.lanes.len() {
                    self.prev[n].push(i);
                }
            }
        }
        self.compute_conflicts();
        self.compute_reach();
        self.compute_rings();
    }

    /// Mark the lanes of roundabouts: short street lanes that lead back to themselves within
    /// `RING_MAX` metres, turning once round the way traffic circulates (anticlockwise, or
    /// clockwise on a left-hand-traffic map). Where the map sets no priorities, traffic on
    /// the ring goes before traffic entering it (`must_yield`), not "from the right".
    pub fn compute_rings(&mut self) {
        let n = self.lanes.len();
        let mut ring = vec![false; n];
        let short = |l: &Lane| l.kind == LaneKind::Street && l.length() <= RING_LANE_MAX;
        // circulating anticlockwise, the compass heading falls by a full turn
        let turn = if self.left_hand { 360.0 } else { -360.0 };
        for start in 0..n {
            if ring[start] || !short(&self.lanes[start]) {
                continue;
            }
            // depth-first along `next`: (lane, path so far, length so far, heading change)
            let first = &self.lanes[start];
            let mut stack = vec![(start, vec![start], first.length(),
                wrap_deg(first.end_heading() - first.start_heading()))];
            let mut found: Option<Vec<usize>> = None;
            // (bounded: a big junction object has hundreds of short paths)
            let mut budget = 4000;
            while let Some((lane, path, len, change)) = stack.pop() {
                budget -= 1;
                if budget == 0 {
                    break;
                }
                for &next in &self.lanes[lane].next {
                    let l = &self.lanes[next];
                    let joint = wrap_deg(l.start_heading() - self.lanes[lane].end_heading());
                    if next == start {
                        if (change + joint - turn).abs() < 60.0 {
                            found = Some(path.clone());
                        }
                        continue;
                    }
                    let total = len + l.length();
                    if !short(l) || total > RING_MAX || path.len() >= 16 || path.contains(&next) {
                        continue;
                    }
                    let mut p = path.clone();
                    p.push(next);
                    stack.push((next, p, total,
                        change + joint + wrap_deg(l.end_heading() - l.start_heading())));
                }
                if found.is_some() {
                    break;
                }
            }
            for i in found.unwrap_or_default() {
                ring[i] = true;
            }
        }
        let count = ring.iter().filter(|&&r| r).count();
        if count > 0 {
            log::info!("path network: {count} lanes run round roundabouts");
        }
        self.ring = ring;
    }

    /// Does `lane` run round a roundabout?
    pub fn is_ring(&self, lane: usize) -> bool {
        self.ring.get(lane).copied().unwrap_or(false)
    }

    /// `reach` of every lane: its length plus the best reach of the lanes after it. The lanes
    /// are settled from the dead ends backwards (a lane once all the lanes after it are); a
    /// lane that never settles has a loop ahead of it and can be driven on for ever.
    pub fn compute_reach(&mut self) {
        let dead = self.update_reach();
        log::info!("path network: {dead} street lanes lead into a dead end within {DEAD_END} m");
    }

    /// `compute_reach` without the log line; returns the street lanes that lead into a dead
    /// end.
    fn update_reach(&mut self) -> usize {
        let n = self.lanes.len();
        // a lane closed to cars is no way on for one that is open
        let counts = |i: usize, j: usize| !self.lanes[j].no_cars || self.lanes[i].no_cars;
        let mut waiting: Vec<usize> = (0..n)
            .map(|i| self.lanes[i].next.iter().filter(|&&j| counts(i, j)).count())
            .collect();
        let mut reach = vec![REACH_MAX; n];
        let mut settled = vec![false; n];
        let mut ready: Vec<usize> = (0..n).filter(|&i| waiting[i] == 0).collect();
        while let Some(j) = ready.pop() {
            let l = &self.lanes[j];
            let best = l
                .next
                .iter()
                .filter(|&&k| counts(j, k))
                .map(|&k| reach[k])
                .fold(0.0f32, f32::max);
            reach[j] = (l.length() + best).min(REACH_MAX);
            settled[j] = true;
            for &p in self.prev.get(j).map(|v| v.as_slice()).unwrap_or(&[]) {
                if counts(p, j) && !settled[p] {
                    waiting[p] = waiting[p].saturating_sub(1);
                    if waiting[p] == 0 {
                        ready.push(p);
                    }
                }
            }
        }
        let dead = self
            .lanes
            .iter()
            .zip(&reach)
            .filter(|(l, r)| l.kind == LaneKind::Street && **r < DEAD_END)
            .count();
        self.reach = reach;
        dead
    }

    /// Nearest street lane and distance along it to a world point.
    /// Sort the lanes into the grid (called by `link`; call again after adding lanes).
    pub fn build_grid(&mut self) {
        self.grid.clear();
        self.start_grid.clear();
        for (i, l) in self.lanes.iter().enumerate() {
            self.start_grid
                .entry(Self::grid_cell(l.start()))
                .or_default()
                .push(i);
            let mut cells: Vec<(i32, i32)> = Vec::new();
            for w in l.points.windows(2) {
                // every cell along the segment, sampled finer than a cell
                let n = ((w[1] - w[0]).truncate().length() / (GRID_CELL * 0.5))
                    .ceil()
                    .max(1.0) as usize;
                for k in 0..=n {
                    let q = w[0].lerp(w[1], k as f64 / n as f64);
                    let c = (
                        (q.x / GRID_CELL).floor() as i32,
                        (q.y / GRID_CELL).floor() as i32,
                    );
                    if !cells.contains(&c) {
                        cells.push(c);
                    }
                }
            }
            if l.points.len() == 1 {
                cells.push((
                    (l.points[0].x / GRID_CELL).floor() as i32,
                    (l.points[0].y / GRID_CELL).floor() as i32,
                ));
            }
            for c in cells {
                self.grid.entry(c).or_default().push(i);
            }
        }
    }

    /// Lane indices whose starts lie in cells intersecting a circle around `p`.
    /// Callers still apply their own exact distance and lane-kind tests. Sorting keeps
    /// their traversal (and seeded traffic selection) in map order.
    pub fn lanes_starting_near(&self, p: DVec3, radius: f64) -> Vec<usize> {
        let min_x = ((p.x - radius) / GRID_CELL).floor() as i32;
        let max_x = ((p.x + radius) / GRID_CELL).floor() as i32;
        let min_y = ((p.y - radius) / GRID_CELL).floor() as i32;
        let max_y = ((p.y + radius) / GRID_CELL).floor() as i32;
        let cells = (max_x as i64 - min_x as i64 + 1) * (max_y as i64 - min_y as i64 + 1);
        if self.start_grid.is_empty() || cells > self.start_grid.len() as i64 * 2 {
            return (0..self.lanes.len()).collect();
        }
        let mut out = Vec::new();
        for x in min_x..=max_x {
            for y in min_y..=max_y {
                if let Some(lanes) = self.start_grid.get(&(x, y)) {
                    out.extend_from_slice(lanes);
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// The nearest lane of `kind` to `p`: (lane, distance along it, distance to it). With
    /// the grid built, only lanes within about a cell of `p` are considered - enough for
    /// everything that asks where a vehicle or a person stands.
    pub fn nearest_lane(&self, p: DVec3, kind: LaneKind) -> Option<(usize, f32, f64)> {
        if !self.grid.is_empty() {
            if let Some(best) = self.nearest_lane_near(p, kind) {
                return Some(best);
            }
        }
        let mut best: Option<(usize, f32, f64)> = None;
        for (i, l) in self.lanes.iter().enumerate() {
            if l.kind != kind {
                continue;
            }
            for k in 0..l.points.len().saturating_sub(1) {
                let a = l.points[k];
                let b = l.points[k + 1];
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
                let q = a + ab * t;
                let d = (q - p).length();
                if best.map(|b| d < b.2).unwrap_or(true) {
                    let s = l.dist[k] + (l.dist[k + 1] - l.dist[k]) * t as f32;
                    best = Some((i, s, d));
                }
            }
        }
        best
    }

    /// Find shortest lane path from `start` to `target` using Dijkstra's algorithm.
    pub fn shortest_path(&self, start: usize, target: usize) -> Option<Vec<usize>> {
        if start >= self.lanes.len() || target >= self.lanes.len() {
            return None;
        }
        if start == target {
            return Some(vec![start]);
        }
        use std::cmp::Ordering;
        use std::collections::BinaryHeap;

        #[derive(Copy, Clone, PartialEq)]
        struct State {
            cost: f32,
            position: usize,
        }
        impl Eq for State {}
        impl Ord for State {
            fn cmp(&self, other: &Self) -> Ordering {
                other.cost.total_cmp(&self.cost)
            }
        }
        impl PartialOrd for State {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        let mut dist = vec![f32::INFINITY; self.lanes.len()];
        let mut parent = vec![None; self.lanes.len()];
        let mut heap = BinaryHeap::new();

        dist[start] = 0.0;
        heap.push(State {
            cost: 0.0,
            position: start,
        });

        while let Some(State { cost, position }) = heap.pop() {
            if position == target {
                let mut path = Vec::new();
                let mut curr = Some(target);
                while let Some(node) = curr {
                    path.push(node);
                    curr = parent[node];
                }
                path.reverse();
                return Some(path);
            }

            if cost > dist[position] {
                continue;
            }

            for &next_idx in &self.lanes[position].next {
                if next_idx >= self.lanes.len() {
                    continue;
                }
                let next_len = self.lanes[next_idx].length();
                let next_cost = cost + next_len;
                if next_cost < dist[next_idx] {
                    dist[next_idx] = next_cost;
                    parent[next_idx] = Some(position);
                    heap.push(State {
                        cost: next_cost,
                        position: next_idx,
                    });
                }
            }
        }
        None
    }
}

/// Growing the network while the map streams in: the lanes of newly loaded tiles are
/// appended (existing indices stay valid for the cars and routes that hold them) and linked
/// to what is there, with the same rules as [`Network::link`].
impl Network {
    /// Append `new` lanes and link them. Returns the range of their indices.
    pub fn extend(&mut self, new: Vec<Lane>, tol: f64) -> std::ops::Range<usize> {
        let first = self.lanes.len();
        if new.is_empty() {
            return first..first;
        }
        if first == 0 {
            self.lanes = new;
            self.link(tol);
            return 0..self.lanes.len();
        }
        self.lanes.extend(new);
        let end = self.lanes.len();
        for i in first..end {
            if let Some(k) = self.lanes[i].key {
                self.by_key.entry(k).or_default().push(i);
            }
            self.start_grid
                .entry(Self::grid_cell(self.lanes[i].start()))
                .or_default()
                .push(i);
            for c in Self::lane_cells(&self.lanes[i]) {
                self.grid.entry(c).or_default().push(i);
            }
        }
        // neighbouring lanes of the new splines
        let mut by_spline: HashMap<((i32, i32), i64, bool), Vec<usize>> = HashMap::new();
        for i in first..end {
            let l = &self.lanes[i];
            if let (Some(k), 1, LaneKind::Street) = (l.key, l.source, l.kind) {
                by_spline
                    .entry((k.tile, k.id, l.reversed))
                    .or_default()
                    .push(i);
            }
        }
        for (_, mut list) in by_spline {
            list.sort_by(|a, b| self.lanes[*a].offset.total_cmp(&self.lanes[*b].offset));
            for w in list.windows(2) {
                let (a, b) = (w[0], w[1]);
                let gap = self.lanes[b].offset - self.lanes[a].offset;
                if gap > 1.5 && gap < 6.0 {
                    if self.lanes[a].reversed {
                        self.lanes[a].left = Some(b);
                        self.lanes[b].right = Some(a);
                    } else {
                        self.lanes[a].right = Some(b);
                        self.lanes[b].left = Some(a);
                    }
                }
            }
        }
        // links: ends of new lanes to any start, ends of old lanes to new starts
        let joins = |net: &Network, from: usize, to: usize| -> bool {
            let (a, b) = (&net.lanes[from], &net.lanes[to]);
            if from == to || a.kind != b.kind {
                return false;
            }
            let (e, s) = (a.end(), b.start());
            let dh = ((b.start_heading() - a.end_heading() + 540.0) % 360.0 - 180.0).abs();
            (s - e).truncate().length() <= tol && dh < 40.0 && (s.z - e.z).abs() < 3.0
        };
        let near = |net: &Network, p: DVec3| -> Vec<usize> {
            let c = (
                (p.x / GRID_CELL).floor() as i32,
                (p.y / GRID_CELL).floor() as i32,
            );
            let mut out: Vec<usize> = Vec::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(list) = net.grid.get(&(c.0 + dx, c.1 + dy)) {
                        out.extend(list.iter().copied());
                    }
                }
            }
            out.sort_unstable();
            out.dedup();
            out
        };
        let mut added: Vec<(usize, usize)> = Vec::new();
        for i in first..end {
            let e = self.lanes[i].end();
            let found: Vec<usize> = near(self, e)
                .into_iter()
                .filter(|&j| joins(self, i, j))
                .collect();
            for &j in &found {
                added.push((i, j));
            }
            self.lanes[i].next = found;
            let s = self.lanes[i].start();
            for j in near(self, s) {
                if j < first && joins(self, j, i) && !self.lanes[j].next.contains(&i) {
                    self.lanes[j].next.push(i);
                    added.push((j, i));
                }
            }
        }
        self.prev.resize(end, Vec::new());
        for (from, to) in added {
            if !self.prev[to].contains(&from) {
                self.prev[to].push(from);
            }
        }
        // meeting places and footpath crossings inside the new junction objects, and how far
        // every lane now leads (a dead end may go on into the new tiles)
        self.conflicts_from(first);
        self.update_reach_from(first);
        self.bump_version();
        first..end
    }

    /// Appending lanes can only change the reach of those lanes and their predecessors.
    /// An outgoing edge into the rest of the network uses its already settled reach. This
    /// avoids rewalking every loaded tile whenever the streamer adds a small batch.
    fn update_reach_from(&mut self, first: usize) {
        if self.reach.len() != first {
            self.update_reach();
            return;
        }
        let mut affected: hashbrown::HashSet<usize> = (first..self.lanes.len()).collect();
        let mut pending: Vec<usize> = affected.iter().copied().collect();
        while let Some(i) = pending.pop() {
            // The vector-based full pass is cheaper once much of the network leads
            // into this tile (for example a strongly connected city grid).
            if affected.len() > self.lanes.len() / 4 {
                self.update_reach();
                return;
            }
            for &p in &self.prev[i] {
                if affected.insert(p) {
                    pending.push(p);
                }
            }
        }
        self.reach.resize(self.lanes.len(), REACH_MAX);
        let counts = |i: usize, j: usize| !self.lanes[j].no_cars || self.lanes[i].no_cars;
        let mut waiting: HashMap<usize, usize> = affected
            .iter()
            .map(|&i| {
                (
                    i,
                    self.lanes[i]
                        .next
                        .iter()
                        .filter(|&&j| affected.contains(&j) && counts(i, j))
                        .count(),
                )
            })
            .collect();
        let mut ready: Vec<usize> = waiting
            .iter()
            .filter_map(|(&i, &n)| (n == 0).then_some(i))
            .collect();
        for &i in &affected {
            self.reach[i] = REACH_MAX;
        }
        while let Some(i) = ready.pop() {
            let l = &self.lanes[i];
            let best = l
                .next
                .iter()
                .filter(|&&j| counts(i, j))
                .map(|&j| self.reach[j])
                .fold(0.0f32, f32::max);
            self.reach[i] = (l.length() + best).min(REACH_MAX);
            for &p in &self.prev[i] {
                if counts(p, i) {
                    if let Some(n) = waiting.get_mut(&p) {
                        *n = n.saturating_sub(1);
                        if *n == 0 {
                            ready.push(p);
                        }
                    }
                }
            }
        }
    }

    /// Like [`nearest_lane`](Self::nearest_lane), but only among the lanes in the grid cells
    /// around `p` (a cell is [`GRID_CELL`]): None when there are none there, never a search
    /// of the whole network.
    pub fn nearest_lane_near(&self, p: DVec3, kind: LaneKind) -> Option<(usize, f32, f64)> {
        let (cx, cy) = Self::grid_cell(p);
        let mut best: Option<(usize, f32, f64)> = None;
        let mut seen: Vec<usize> = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(list) = self.grid.get(&(cx + dx, cy + dy)) else {
                    continue;
                };
                for &i in list {
                    if seen.contains(&i) {
                        continue;
                    }
                    seen.push(i);
                    let l = &self.lanes[i];
                    if l.kind != kind {
                        continue;
                    }
                    if let Some((s, d)) = l.nearest_point(p) {
                        if best.map(|b| d < b.2).unwrap_or(true) {
                            best = Some((i, s, d));
                        }
                    }
                }
            }
        }
        best
    }

    /// The nearest lane of `kind` within `max_dist` of `p` that runs within `max_turn`
    /// degrees of `heading` there (a vehicle's own lane, not the one beside it going the
    /// other way): (lane, distance along it, distance to it).
    pub fn lane_along(
        &self,
        p: DVec3,
        heading: f64,
        kind: LaneKind,
        max_dist: f64,
        max_turn: f64,
    ) -> Option<(usize, f32, f64)> {
        let (cx, cy) = Self::grid_cell(p);
        let mut best: Option<(usize, f32, f64)> = None;
        let mut seen: Vec<usize> = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(list) = self.grid.get(&(cx + dx, cy + dy)) else {
                    continue;
                };
                for &i in list {
                    if seen.contains(&i) {
                        continue;
                    }
                    seen.push(i);
                    let l = &self.lanes[i];
                    if l.kind != kind {
                        continue;
                    }
                    let Some((s, d)) = l.nearest_point(p) else {
                        continue;
                    };
                    if d > max_dist || best.map(|b| d >= b.2).unwrap_or(false) {
                        continue;
                    }
                    let turn = (l.at(s).1 as f64 - heading + 540.0).rem_euclid(360.0) - 180.0;
                    if turn.abs() <= max_turn {
                        best = Some((i, s, d));
                    }
                }
            }
        }
        best
    }

    /// The grid cell `p` lies in.
    pub fn grid_cell(p: DVec3) -> (i32, i32) {
        (
            (p.x / GRID_CELL).floor() as i32,
            (p.y / GRID_CELL).floor() as i32,
        )
    }

    /// The grid cells a lane's points touch (see [`Network::build_grid`]).
    pub fn lane_cells(l: &Lane) -> Vec<(i32, i32)> {
        let mut cells: Vec<(i32, i32)> = Vec::new();
        for w in l.points.windows(2) {
            let n = ((w[1] - w[0]).truncate().length() / (GRID_CELL * 0.5))
                .ceil()
                .max(1.0) as usize;
            for k in 0..=n {
                let q = w[0].lerp(w[1], k as f64 / n as f64);
                let c = (
                    (q.x / GRID_CELL).floor() as i32,
                    (q.y / GRID_CELL).floor() as i32,
                );
                if !cells.contains(&c) {
                    cells.push(c);
                }
            }
        }
        if l.points.len() == 1 {
            cells.push((
                (l.points[0].x / GRID_CELL).floor() as i32,
                (l.points[0].y / GRID_CELL).floor() as i32,
            ));
        }
        cells
    }
}

#[cfg(test)]
mod extend_tests {
    use super::*;

    fn straight(x: f64, y0: f64, y1: f64, id: i64, tile: (i32, i32)) -> Lane {
        let mut l = LaneBuilder::polyline(
            vec![DVec3::new(x, y0, 0.0), DVec3::new(x, y1, 0.0)],
            LaneKind::Street,
            3.0,
        );
        l.key = Some(LaneKey { tile, id, path: 0 });
        l.source = 1;
        l
    }

    #[test]
    fn extend_links_like_link() {
        let all = vec![
            straight(0.0, 0.0, 100.0, 1, (0, 0)),
            straight(0.0, 100.0, 200.0, 2, (0, 0)),
            straight(0.0, 200.0, 300.0, 3, (0, 1)),
            straight(0.0, 300.0, 400.0, 4, (0, 1)),
        ];
        let mut whole = Network {
            lanes: all.clone(),
            ..Default::default()
        };
        whole.link(1.5);
        let mut grown = Network {
            lanes: all[..2].to_vec(),
            ..Default::default()
        };
        grown.link(1.5);
        let r = grown.extend(all[2..].to_vec(), 1.5);
        assert_eq!(r, 2..4);
        for i in 0..4 {
            assert_eq!(grown.lanes[i].next, whole.lanes[i].next, "lane {i}");
            assert_eq!(grown.prev[i], whole.prev[i], "lane {i}");
        }
        assert_eq!(grown.reach, whole.reach);
        assert_eq!(grown.crossings.len(), 4);
        assert_eq!(
            grown.find(
                LaneKey {
                    tile: (0, 1),
                    id: 3,
                    path: 0
                },
                None
            ),
            Some(2)
        );
        assert_eq!(
            grown
                .nearest_lane(DVec3::new(0.5, 350.0, 0.0), LaneKind::Street)
                .map(|n| n.0),
            Some(3)
        );
    }

    #[test]
    fn start_grid_tracks_added_lanes_in_map_order() {
        let first = vec![
            straight(0.0, 0.0, 100.0, 1, (0, 0)),
            straight(300.0, 0.0, 100.0, 2, (1, 0)),
        ];
        let mut net = Network {
            lanes: first,
            ..Default::default()
        };
        net.link(1.5);
        let near = |net: &Network| {
            net.lanes_starting_near(DVec3::ZERO, 60.0)
                .into_iter()
                .filter(|&i| net.lanes[i].start().truncate().length() < 60.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(near(&net), vec![0]);
        net.extend(vec![straight(25.0, 0.0, 100.0, 3, (0, 1))], 1.5);
        assert_eq!(near(&net), vec![0, 2]);
    }

    #[test]
    fn added_lanes_only_change_reach_of_their_predecessors() {
        let old = vec![
            straight(0.0, 0.0, 100.0, 1, (0, 0)),
            straight(0.0, 100.0, 200.0, 2, (0, 0)),
            straight(500.0, 0.0, 100.0, 3, (2, 0)),
        ];
        let extra = vec![straight(0.0, 200.0, 300.0, 4, (0, 1))];
        let mut grown = Network {
            lanes: old.clone(),
            ..Default::default()
        };
        grown.link(1.5);
        let distant_reach = grown.reach[2];
        grown.extend(extra.clone(), 1.5);
        let mut whole = Network {
            lanes: old.into_iter().chain(extra).collect(),
            ..Default::default()
        };
        whole.link(1.5);
        assert_eq!(grown.reach, whole.reach);
        assert_eq!(grown.reach[2], distant_reach);
    }
}

#[cfg(test)]
mod aurora_pool_tests {
    use super::*;
    #[test]
    fn independent_rules_and_default_references() {
        let mut lane = LaneBuilder::arc(DVec3::ZERO, 0.0, 100.0, 0.0, 0.0, LaneKind::Street, 3.0);
        lane.group_density = vec![(0, 1.0), (1, 0.1), (3, 0.001), (5, 0.0)];
        let defaults = [1, 0, 1, 0, 0, 0];
        assert_eq!(lane.pool_density(&defaults, 1), 0.1);
        assert_eq!(lane.pool_density(&defaults, 2), 1.0);
        assert_eq!(lane.pool_density(&defaults, 3), 0.001);
        assert_eq!(lane.pool_density(&defaults, 4), 0.0);
        assert_eq!(lane.pool_density(&defaults, 5), 0.0);
        lane.group_density.push((1, 0.5));
        assert_eq!(lane.pool_density(&defaults, 1), 0.5);
    }
}
