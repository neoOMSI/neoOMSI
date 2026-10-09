//! Scheduled AI buses: the map's timetable lines (`TTData`) put buses on their tracks at the
//! tour departure times; they follow the track lanes and stop at the trip's stations.

use crate::scene::World;
use crate::traffic::Traffic;
use hashbrown::{HashMap, HashSet};
use ::render::{Renderer, Scene};
use ::simulation::VehicleType;
use ::traffic::{
    LaneId, LaneKey, Network, Reason, RouteStatus, RouteStepState, TileState, bridge_gaps,
    compile_route, joins, way_between,
};
use std::path::Path;
use std::sync::Arc;
use ::timetable::TimetableData;

struct Departure {
    /// Seconds since midnight.
    time: f64,
    trip: usize,
    /// The trip's profile the tour runs it with (`[addtrip]`).
    profile: usize,
    line: String,
    ai_group: String,
    tour: String,
    /// The tour's validity mask (bits 0-6 Monday..Sunday, 7 public holiday, 8 school
    /// holidays, 9 school days): every tour's departures are kept, and which run is decided
    /// by the day (`Schedule::runs`), so a session carries on past midnight.
    mask: i32,
    spawned: bool,
}

/// One step of a trip's route: a lane in map terms (None when the tile index is not in the
/// map's list) and the leg between two stations it belongs to (0 for a track).
#[derive(Debug, Clone, Copy)]
struct Step {
    key: Option<LaneKey>,
    leg: usize,
    /// The path's length as the timetable file has it (m).
    length: f64,
}

/// A route step as the loaded network has it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Slot {
    /// The lane it runs on.
    Lane(usize),
    /// Its tile is part of the map, but has not brought its lanes yet.
    Waiting,
    /// Not in the map (a tile or path that does not exist): passed over, as a whole-map load
    /// passes it over.
    Absent,
}

/// What became of a departure that was due.
enum Placed {
    Spawned,
    /// The part of the route the bus is on now is not loaded: try again later.
    Wait,
    /// Nothing to do any more (the trip is over, or has no route or no vehicle).
    Drop,
    /// A car stands where the bus would appear: try again at the next call.
    Busy,
}

/// A scheduled bus whose route stops short of a tile that has not brought its lanes yet: the
/// route is carried on as the tiles come.
struct RunningTrip {
    car: u64,
    steps: Vec<Step>,
    /// The first step its route does not have yet.
    next: usize,
    /// The trip's stations with their departure times, and which of them the bus stops at
    /// already or has passed.
    stations: Vec<(i64, f64)>,
    served: Vec<bool>,
}

/// When a trip's bus is at each of its stations, as OMSI's timetable has it: the profile
/// gives the trip's duration and, for some stations, the minute the bus arrives or leaves
/// (`[profile_man_arr_time]`, `[profile_man_dep_time]`, minutes after the trip's start);
/// the stations in between are timed by the lengths of the station links (Spandau's line 5
/// gives nearly every station its minute; these used to be ignored for the whole duration
/// split by the link lengths, and the tours ran their first profile whatever `[addtrip]`
/// said). A station marked `[profile_otherstopping] 2` is passed without a stop (every
/// station of a depot run).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TripTimes {
    /// Seconds after the trip's departure: (arrival, departure) per station.
    pub stations: Vec<(f64, f64)>,
    /// Whether the bus stops at the station.
    pub stops: Vec<bool>,
    /// `[profile_otherstopping]` per station (0 when not given): 1 and 4 stop whoever
    /// wants to get on or off, 2 is passed, 3 is served when the bus would be more than 20 s
    /// early (Omsi.exe 0x7da6f0 .. 0x7da8bf; see `traffic::service::ServiceCoordinator`).
    pub kinds: Vec<u8>,
    /// Seconds from the departure to the arrival at the last station.
    pub duration: f64,
}

impl TripTimes {
    pub fn new(
        stations: &[i64],
        profile: Option<&::timetable::TripProfile>,
        link_length: &dyn Fn(i64, i64) -> Option<f64>,
    ) -> TripTimes {
        let n = stations.len();
        let duration = profile
            .map(|p| p.factor as f64 * 60.0)
            .filter(|d| *d > 0.0)
            .unwrap_or(600.0);
        let mut arr: Vec<Option<f64>> = vec![None; n];
        let mut dep: Vec<Option<f64>> = vec![None; n];
        let mut stops = vec![true; n];
        let mut kinds = vec![0u8; n];
        if let Some(p) = profile {
            let at = |i: i32| usize::try_from(i).ok().filter(|i| *i < n);
            for (i, m) in &p.man_arr_time {
                if let Some(i) = at(*i) {
                    arr[i] = Some(*m as f64 * 60.0);
                }
            }
            for (i, m) in &p.man_dep_time {
                if let Some(i) = at(*i) {
                    dep[i] = Some(*m as f64 * 60.0);
                }
            }
            for (i, v) in &p.other_stopping {
                if let Some(i) = at(*i) {
                    kinds[i] = (*v).clamp(0, 255) as u8;
                    if *v == 2 {
                        stops[i] = false;
                    }
                }
            }
        }
        // the trip leaves its first station at its departure and reaches the last one after
        // the profile's duration, unless the profile times them itself
        if n > 0 && arr[0].is_none() && dep[0].is_none() {
            dep[0] = Some(0.0);
        }
        if n > 1 && arr[n - 1].is_none() && dep[n - 1].is_none() {
            arr[n - 1] = Some(duration);
        }
        // the distance to every station along the links (a missing link counts 500 m)
        let mut along = vec![0.0f64; n];
        for i in 1..n {
            along[i] = along[i - 1]
                + link_length(stations[i - 1], stations[i])
                .unwrap_or(500.0)
                .max(1.0);
        }
        let timed: Vec<usize> = (0..n)
            .filter(|&i| arr[i].is_some() || dep[i].is_some())
            .collect();
        let mut out = Vec::with_capacity(n);
        let mut last = 0.0f64;
        for i in 0..n {
            let (a, d) = match (arr[i], dep[i]) {
                (Some(a), Some(d)) => (a, d.max(a)),
                (Some(a), None) => (a, a),
                (None, Some(d)) => (d, d),
                (None, None) => {
                    let p = timed.iter().rev().find(|&&k| k < i).copied();
                    let q = timed.iter().find(|&&k| k > i).copied();
                    let t = match (p, q) {
                        (Some(p), Some(q)) => {
                            let t0 = dep[p].or(arr[p]).unwrap_or(0.0);
                            let t1 = arr[q].or(dep[q]).unwrap_or(t0);
                            let span = along[q] - along[p];
                            if span > 0.0 {
                                t0 + (t1 - t0) * (along[i] - along[p]) / span
                            } else {
                                t0
                            }
                        }
                        (Some(p), None) => dep[p].or(arr[p]).unwrap_or(0.0),
                        (None, Some(q)) => arr[q].or(dep[q]).unwrap_or(0.0),
                        (None, None) => 0.0,
                    };
                    (t, t)
                }
            };
            // never back in time
            let a = a.max(last);
            let d = d.max(a);
            last = d;
            out.push((a, d));
        }
        // a trip with a route of stations lasts until it reaches the last one
        let duration = if n > 1 { out[n - 1].0 } else { duration };
        TripTimes {
            stations: out,
            stops,
            kinds,
            duration: duration.max(1.0),
        }
    }
}

/// The stations a trip calls at: its `[station_typ2]` objects, or the objects of the older
/// `[station]` records the trains, the ferry and the U-Bahn of Spandau still use (their first
/// line is the object id).
/// The station targets of [`Schedule::stop_targets`] from the trips' stops and termini.
/// A stop is never a target of itself or of another stop of the same name (the platforms
/// of one station, the first and last stop of a circular line): somebody waiting there who
/// drew it got in, found the bus at their stop and got straight off again, over and over,
/// every one of them adding another pedestrian (#795).
fn station_targets(
    trips: impl Iterator<Item=(Vec<i64>, String)>,
    name_of: impl Fn(i64) -> String,
) -> HashMap<i64, Vec<(String, HashSet<String>)>> {
    let mut named: HashMap<i64, Vec<(String, HashSet<String>)>> = HashMap::new();
    for (stations, terminus) in trips {
        for (k, from) in stations.iter().enumerate() {
            let here = name_of(*from);
            let targets = named.entry(*from).or_default();
            for to in &stations[k + 1..] {
                let to = name_of(*to);
                if to == here {
                    continue;
                }
                match targets.iter_mut().find(|t| t.0 == to) {
                    Some(t) => {
                        t.1.insert(terminus.clone());
                    }
                    None => targets.push((to, HashSet::from_iter([terminus.clone()]))),
                }
            }
        }
    }
    named
}

fn trip_stations(trip: &::timetable::Trip) -> Vec<i64> {
    if !trip.stations.is_empty() {
        return trip.stations.clone();
    }
    trip.stations_legacy
        .iter()
        .filter_map(|s| s.first().and_then(|id| id.trim().parse::<i64>().ok()))
        .collect()
}

/// How far from a route a bus stop may stand when the route is only a part of the trip (a
/// stop of the missing part would otherwise be put on the nearest point of this one).
const STOP_REACH: f64 = 25.0;

/// How far ahead (s) the timetable reads and uploads the vehicles of its next departures: a
/// layover bus stands at its first stop a quarter of an hour early.
const FLEET_AHEAD: f64 = 25.0 * 60.0;
/// [`FLEET_AHEAD`], or `OMSI_FLEET_AHEAD` minutes (for tests).
fn fleet_ahead() -> f64 {
    ::legacy_config::env::var("OMSI_FLEET_AHEAD")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|m| m * 60.0)
        .unwrap_or(FLEET_AHEAD)
}

/// A vehicle set nobody has drawn for this long, and that no departure of the next
/// minutes wants, leaves the GPU.
const FLEET_IDLE: std::time::Duration = std::time::Duration::from_secs(90);

/// The vehicle a departure is driven with. A tour keeps its bus all day, as in OMSI: the
/// choice comes from the tour, not from the order the departures happen to spawn in, so the
/// timetable knows which vehicles its next minutes need before they are due.
struct Choice {
    ty: Arc<VehicleType>,
    number: Option<(String, String)>,
    hof: Option<Arc<::legacy_vehicle::Hof>>,
    scheme: Option<usize>,
    /// A `.zug` train: its cars, the first one being `ty`.
    train: Option<Vec<(Arc<VehicleType>, bool)>>,
}

/// The bus stands next to the kerb: the pole's offset less half a bus width and a gap; only
/// where the pole is clearly off the lane (a bay).
///
/// A pole further off than a bay's width stands behind the pavement or the verge (the
/// stop objects of many maps are placed there): the bus stays in its lane at the kerb then.
/// Taken as a bay up to 4 m wide, the bus pulled out over the kerb onto the grass.
///
/// Not at all, now: a timetable bus stays on its path at the stop, as OMSI's do (a
/// map's bus bay is a spline of its own that the route runs through). The pole's offset
/// says nothing about where the kerb is - most stand behind the pavement - and a bus
/// moved 1.6 m to the right of its lane drove along with its right wheels on the pavement.
/// Stops moved `shift` metres back along `route` (the lanes the stops' route indices less
/// `base` count in): where the vehicle's origin comes to rest (`bus_service::stop_shift`).
/// One that comes to lie before the route's first lane keeps a distance below zero on it.
fn shift_stops(
    net: &Network,
    route: &[usize],
    base: usize,
    stops: &mut [(usize, f32, f32, f64, i64, f32)],
    shift: f32,
) {
    if shift.abs() < 1e-3 {
        return;
    }
    for st in stops.iter_mut() {
        let (mut k, mut ss) = (st.0.saturating_sub(base), st.1 - shift);
        while ss < 0.0 && k > 0 && k <= route.len() - 1 {
            k -= 1;
            ss += net.lanes[route[k]].length();
        }
        while ss > 0.0 && k + 1 < route.len() && ss > net.lanes[route[k]].length() {
            ss -= net.lanes[route[k]].length();
            k += 1;
        }
        st.0 = base + k;
        st.1 = ss;
    }
}

fn bay_offset(lat: f32) -> f32 {
    lat
}

/// Where a timetable bus stands across its lane at a stop, as Omsi.exe puts it
/// (0x7dac5e..0x7dae81): its kerb-side flank 0.3 m past the `[busstop]` box's centre -
/// `lat` less half its `[boundingbox]` width plus 0.3 on the right (the other way round
/// where traffic keeps left), from the box's offset `lat` off the path (right positive);
/// a railway vehicle keeps to its track. OMSI clamps it only to the room beside other
/// vehicles, not to a kerb: the bus pulls into the bay whether or not a path leads there
/// (#241). (neoOMSI kept it on its path before - a map whose box stood behind the
/// pavement had its buses on the pavement - but OMSI does the same there.)
fn bay_for(lat: f32, ty: &::simulation::VehicleType, rail: bool, left_hand: bool) -> f32 {
    if rail || !lat.is_finite() {
        return 0.0;
    }
    let hw = ty.def.bounding_box.map(|b| b[0] * 0.5).unwrap_or(1.25);
    if left_hand {
        lat + hw - 0.3
    } else {
        lat - hw + 0.3
    }
}

/// The stops' raw box offsets (see `bay_offset`) made the vehicle's bay offsets, and the
/// stops moved to where its origin comes to rest (`shift_stops`).
fn place_stops(
    net: &Network,
    route: &[usize],
    base: usize,
    stops: &mut [(usize, f32, f32, f64, i64, f32)],
    ty: &::simulation::VehicleType,
    rail: bool,
) {
    for st in stops.iter_mut() {
        st.2 = bay_for(st.2, ty, rail, net.left_hand);
    }
    shift_stops(
        net,
        route,
        base,
        stops,
        crate::bus_service::stop_shift(ty, rail),
    );
}

/// Where on `route` the bus stop at `pos` is: (route index, distance along that lane, lateral
/// offset); None when it is further than `reach` from the route.
/// Where the stop at `pos` lies on `route`, not before route index `from` (the stops come
/// in the trip's order): on the side of the road it stands, see
/// `Network::project_stop_on_route`.
fn project_stop(
    net: &Network,
    route: &[usize],
    pos: glam::DVec3,
    reach: Option<f64>,
    from: usize,
) -> Option<(usize, f32, f32)> {
    net.project_stop_on_route(route, pos, reach, from)
}

pub struct Schedule {
    pub data: TimetableData,
    departures: Vec<Departure>,
    /// Depot vehicles per AI group: (type, its fleet from the ailists, depot file).
    depots: HashMap<
        String,
        Vec<(
            Arc<VehicleType>,
            Vec<::map::DepotEntry>,
            Option<Arc<::legacy_vehicle::Hof>>,
        )>,
    >,
    tile_coords: Vec<(i32, i32)>,
    next_number: usize,
    /// Trains per AI group: list of (car type, reversed), first car leads.
    trains: HashMap<String, Vec<Vec<(Arc<VehicleType>, bool)>>>,
    /// Plain `[aigroup_2]` vehicle pools, loaded the first time a trip asks for one
    /// (the Tegel approaches are flown by the group's own aircraft, not by depot buses).
    pools: HashMap<String, Vec<Arc<VehicleType>>>,
    /// Departures that are due but not on the road yet. Putting twenty minutes of a Berlin
    /// timetable on the map at once costs several seconds in one frame, so they are spawned
    /// a few at a time.
    pending: std::collections::VecDeque<usize>,
    /// Per departure: the previous departure of the same tour (its bus is the same one).
    tour_prev: Vec<Option<usize>>,
    /// Due departures whose bus would be on a part of its route that is not loaded: tried
    /// again when tiles bring lanes and as time moves the bus on.
    waiting: Vec<usize>,
    /// Buses on the road whose route is still to be carried on.
    running: Vec<RunningTrip>,
    /// `Traffic::lanes_generation` when the waiting departures and the running routes were
    /// last looked at, and the time of day of the last retry.
    seen_generation: u64,
    last_retry: f64,
    /// The departure each timetable bus on the road runs (by car id): a bus the traffic took
    /// off with its unloaded tile goes back to `waiting` and returns with the tiles, at the
    /// place its timetable puts it then.
    car_departure: HashMap<u64, usize>,
    /// Waiting departures whose vehicle is timed to reach loaded lanes at this time of day:
    /// they are tried again then, so that a plane coming in over tiles nobody loads appears
    /// where its path enters the loaded ones, not up to half a minute later in mid-air.
    retry_at: HashMap<usize, f64>,
    /// Vehicle sets being read ahead on the workers, with the type to upload them with, and
    /// those read and waiting for their upload.
    fleet_reading: HashMap<crate::scene::VehicleKey, Arc<VehicleType>>,
    fleet_ready: Arc<parking_lot::Mutex<Vec<crate::scene::VehicleKey>>>,
    /// Time of day of the last look at the next departures' vehicles.
    fleet_check: f64,
    /// Per trip and profile: when its bus is at its stations.
    times: Vec<Vec<TripTimes>>,
    /// Per bus stop (map object id): the trips that call there, as (trip, station index).
    visits: HashMap<i64, Vec<(usize, usize)>>,
    /// Per trip: today's departures that run it.
    trip_departures: Vec<Vec<usize>>,
    /// When the departure boards were last made (time of day).
    boards_made: f64,
    /// The tour the player drives (line, tour): the timetable does not run it as well.
    player_tour: Option<(String, String)>,
    /// With a single trip picked: the departure (s of the day) of that trip; the rest of
    /// the tour stays the AI's.
    player_departure: Option<f64>,
    /// A player tour was just taken over: its buses already on the road go at the next tick.
    purge_player_tour: bool,
    /// LAN play (host): the tours the other players drive (line, tour; lower case), left to
    /// them like our own.
    lan_tours: HashSet<(String, String)>,
    /// Time of today's timetable at the last tick (s).
    last_tod: f64,
    /// The lines the date's chrono folders take off the timetable, with the folder that does
    /// it: why a duty on such a line cannot be driven.
    deactivated: Vec<(String, std::path::PathBuf)>,
    /// Stations some trip stops at on its way or ends at (not only starts from).
    served: std::collections::HashSet<i64>,
    /// Per first station: whether another trip's bus stops there or within a bus length
    /// or two of it (maps often put one stop object per line at the same kerb), once its
    /// position is known.
    shared_stand: HashMap<i64, bool>,
    /// Departures queued while the map loads: their buses may appear in view.
    startup: std::collections::HashSet<usize>,
    /// Layover departures whose stand was taken: they come at their departure time.
    later_layover: std::collections::HashSet<usize>,
    /// Per departure: the next departure of the same tour (its bus takes it on).
    tour_next: Vec<Option<usize>>,
    /// Departures due while their tour's bus is still on its previous trip: that bus takes
    /// them on when it gets there, as in OMSI a tour keeps its bus from trip to trip.
    awaiting: std::collections::HashSet<usize>,
    /// The map's holidays, for the day's tours.
    calendar: ::map::Calendar,
    /// The date (yyyymmdd) the departures are for, and the mask bits it selects (day,
    /// school); `set_day` moves them on at midnight.
    day: i32,
    day_bits: (i32, i32),
    /// The weekday bit of the next day (night tours run on into it).
    next_day_bit: i32,
    /// Where today's midnight lies on the traffic's clock (`Traffic::day_time` counts on past
    /// 24:00): a departure leaves at `day_base + time` (`dep_time`), and the date moves on
    /// when the clock passes the next midnight.
    day_base: f64,
    /// The current date (its time of day is not used).
    date_clock: ::simulation::SimClock,
    /// The map's `car_use/*.ocu`: which vehicles serve which line's tours.
    car_use: Vec<::timetable::CarUse>,
    /// Per tour (`tour_key_of`): the depot vehicle (index in its group) and fleet number
    /// it runs with today - from `car_use`, else drawn the first time the tour is due. A
    /// number is given to one tour only (`used_numbers`), as in OMSI:
    /// hashing each tour to a number put the same fleet number on two buses at once.
    tour_vehicle: HashMap<u64, (usize, usize)>,
    used_numbers: HashSet<(String, String)>,
}

/// A tour's bus waits at the end of a trip for the next one of its tour when that leaves
/// within this many seconds; for a longer break it goes (and a bus comes back for it).
const TOUR_LAYOVER_MAX: f64 = 30.0 * 60.0;

/// How early a bus waits at its first stop for its departure (s): a quarter of an hour at a
/// stand of its own, a minute where other buses stop as well (a layover bus there made
/// every bus of the other lines queue behind it until it left).
const LAYOVER: f64 = 900.0;
const LAYOVER_SHARED: f64 = 60.0;


mod duty;
mod fleet;
mod ibis;
mod player_duty;
mod routing;
mod setup;
mod slots;
mod spawn;
mod tick;
mod tours;
mod util;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
use self::routing::*;
#[allow(unused_imports)]
pub use self::ibis::*;
#[allow(unused_imports)]
pub use self::util::*;

/// One stop of a planned trip with its scheduled times (seconds since midnight).
#[derive(Debug, Clone)]
pub struct PlannedStop {
    pub object_id: i64,
    pub name: String,
    pub arr: f64,
    pub dep: f64,
    pub position: Option<glam::DVec3>,
    /// Which way the trip runs through the stop ([`StopDir`]): a circular route, or one
    /// that turns back, calls at the same place twice and the two stops of it stand a few
    /// metres apart. Only the direction says which of them a bus has reached (#254).
    pub dir: StopDir,
    /// The bus stops here (a depot run passes its stations).
    pub stops: bool,
}

/// Which way a trip runs through one of its stops: the direction it arrives on and the one
/// it leaves on, as unit vectors of the ground plane (x east, y north). None where a
/// neighbour's place is unknown or too near to tell a direction - any heading will do then.
#[derive(Debug, Clone, Copy, Default)]
pub struct StopDir {
    pub inbound: Option<glam::DVec2>,
    pub outbound: Option<glam::DVec2>,
}

impl StopDir {
    fn takes(self, fwd: glam::DVec2) -> bool {
        if self.inbound.is_none() && self.outbound.is_none() {
            return true;
        }
        [self.inbound, self.outbound]
            .into_iter()
            .flatten()
            .any(|d| fwd.dot(d) >= DIR_COS)
    }
}

/// The unit vector of the ground plane a bus heading `deg` drives along (degrees clockwise
/// from north, as `VehicleInstance::heading`).
fn forward_of(deg: f64) -> glam::DVec2 {
    let h = deg.to_radians();
    glam::DVec2::new(h.sin(), h.cos())
}

/// How far apart two stops of a trip must stand before the line between them is taken as
/// the way the trip runs between them (m).
const DIR_REACH: f64 = 20.0;
/// How far off the way a trip runs through a stop a bus may head and still be taken as
/// running that way: the cosine of the angle, 60 degrees either side.
const DIR_COS: f64 = 0.5;

#[derive(Debug, Clone)]
pub struct PlannedTrip {
    pub name: String,
    pub line: String,
    pub terminus: String,
    pub departure: f64,
    /// Arrival at the last station.
    pub end: f64,
    pub stops: Vec<PlannedStop>,
}

impl PlannedTrip {
    /// Give every stop the way the trip runs through it: in from the stop before, out to
    /// the stop after.
    fn set_dirs(&mut self) {
        let p: Vec<Option<glam::DVec3>> = self.stops.iter().map(|s| s.position).collect();
        let dir = |a: Option<glam::DVec3>, b: Option<glam::DVec3>| -> Option<glam::DVec2> {
            let v = (b? - a?).truncate();
            (v.length() >= DIR_REACH).then(|| v.normalize())
        };
        for (i, s) in self.stops.iter_mut().enumerate() {
            s.dir = StopDir {
                inbound: i.checked_sub(1).and_then(|k| dir(p[k], p[i])),
                outbound: p.get(i + 1).and_then(|b| dir(p[i], *b)),
            };
        }
    }
}

/// The bus is at a stop within this distance (m), and has left it beyond the second.
const AT_STOP: f64 = 25.0;
const LEFT_STOP: f64 = 35.0;

/// How far ahead (s) the departure displays look.
const BOARD_AHEAD: f64 = 2.0 * 3600.0;
/// Most departures a page gets for a stop (`omsi.getDepartures`).
const MAX_PAGE_DEPARTURES: usize = 20;

/// Where timetable stop `k` (called `name`) stands in the IBIS's own stop list of route
/// `route` (an index into the depot file's `info_busstop_lists`): the stop of that name
/// nearest `k`. What `IBIS_busstop` has to be for the IBIS to show that stop.
pub fn ibis_stop_index(
    hof: &::legacy_vehicle::Hof,
    route: usize,
    name: &str,
    k: usize,
) -> Option<usize> {
    let stop = (name.trim().to_lowercase(), stop_words(name));
    let list = hof.info_busstop_lists.get(route)?;
    // (spelt as `pick_route` compares the names: "Kirchweg" is the depot file's "F_Kirchweg")
    list.iter()
        .enumerate()
        .filter(|(_, id)| same_stop(&ident_names(hof, id), &stop))
        .map(|(i, _)| i)
        .min_by_key(|i| i.abs_diff(k))
}

/// The player's tour: its trips with planned stop times, and the progress along them.
pub struct PlayerDuty {
    pub line: String,
    pub tour: String,
    pub trips: Vec<PlannedTrip>,
    pub trip_index: usize,
    /// Where `trips` begins in the tour: a picked trip is a duty of its own, and a saved
    /// situation counts the trip under way from the tour's first.
    pub first_trip: usize,
    pub(crate) statistics: crate::run_statistics::TripLog,
    completed_report: Option<crate::run_statistics::Report>,
    /// Next stop to serve on the current trip.
    pub next_stop: usize,
    /// True while the bus stands at the next stop.
    at_stop: bool,
    /// How late the bus arrived at the stop it stands at (s after its arrival time).
    arrived_late: Option<f64>,
    /// The bus has reached the last stop of the current trip.
    done: bool,
    /// How late (s, negative = early) the bus left the last stop it served on this trip;
    /// None while it has not left one.
    left_late: Option<f64>,
    /// A page moved the duty back to an earlier stop: `catch_up` must not jump forward
    /// again to a later stop the bus still stands at, until the bus reaches a stop again.
    held_back: bool,
    /// Path driven (m): the sum of the bus's moves, last position, and the readings at the
    /// arrival at / leaving of the stop. Stops are advanced by this, not by the straight line.
    odo: f64,
    last_pos: Option<glam::DVec3>,
    arrival_odo: f64,
    left_odo: f64,
    /// The first update looks where the bus stands.
    placed: bool,
    /// The current trip changed since the last `take_trip_change`.
    trip_changed: bool,
    /// The player picked the current trip: the duty does not move on past it before it is
    /// driven (or given up), however late the bus is for it.
    picked: bool,
    /// Time of day of the first update (placing waits a little for the places of stops
    /// beyond the loaded tiles, see `learn_places`).
    first_update: Option<f64>,
    /// The way the bus faces (degrees clockwise from north), from the last update: it says
    /// which of two stops a few metres apart the bus is at (see `StopDir`).
    heading: f64,
}


/// Where a timetable bus on the road is in its trip, for the departure boards.
struct OnRoad {
    /// Timetable departure time of its next stop (None: it has served its last).
    next: Option<f64>,
    /// Standing at that stop.
    dwelling: bool,
    /// How late it left its last stop (s).
    late: f64,
}
