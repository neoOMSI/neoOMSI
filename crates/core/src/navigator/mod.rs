use glam::{DMat3, DVec2, DVec3, Mat4, Vec2, Vec3};
use hashbrown::HashMap;
use ::render::{Renderer, Scene, TextureId};
use ::simulation::traffic::{LaneKey, LaneKind, Network};
use ::user_interface::paint::Align;
use ::user_interface::{Atlas, Color, Draw, Fonts, Gpu, Layer, Painter, Rect, Weight};

use crate::traffic::Traffic;

mod api;
mod city;
mod draw;
mod follow;
mod graph;
mod map_view;
mod marks;
mod roads;
mod route;
mod shot;
mod streets;
mod style;
mod surfaces;
#[cfg(test)]
mod tests;
mod util;
mod words;

pub(crate) use self::roads::{confirm_road_surfaces, road_geometry, simplify};
pub(crate) use self::route::way_back;
pub(crate) use self::graph::RoadGraph;
#[cfg(test)]
use self::graph::convex_hull;
use self::{roads::*, route::*, streets::*, style::*, surfaces::*, util::*, words::*};

pub(crate) fn stop_requested(vehicle: &::simulation::vehicle::VehicleInstance) -> bool {
    ["haltewunsch", "haltewunschlampe"]
        .iter()
        .any(|name| vehicle.var(name).is_some_and(|v| v > 0.5))
}

#[derive(Debug, Clone)]
pub struct NavStop {
    pub object_id: i64,
    pub position: DVec3,
    pub name: String,
    pub arrival: f64,
}

pub struct NavFrame<'a> {
    pub traffic: Option<&'a Traffic>,
    pub bus: DVec3,
    pub heading: f64,
    pub speed_kmh: f32,
    pub outside_temp: f32,
    pub inside_temp: f32,
    pub line: Option<String>,
    pub terminus: Option<String>,
    pub stops: Vec<NavStop>,
    pub delay: Option<f64>,
    pub passengers: Option<usize>,
    pub stop_requested: bool,
    pub time: f64,
    pub weekday: i32,
    pub language: &'a str,
    pub units: &'a str,
    pub screen: (f32, f32),
    pub ui_scale: f32,
    pub follow_window: bool,
    pub dt: f32,
}

struct Words {
    kmh: String,
    days: [String; 7],
    off_route: String,
    rerouting: String,
    recalculated: String,
    jam: String,
    slow: String,
    map: String,
    last_stop: String,
    on_time: String,
}

#[derive(Default)]
struct Route {
    key: String,
    lanes: Vec<usize>,
    complete: bool,
    generation: u64,
    progress: usize,
    s: f32,
    on_route: bool,
    off_for: f32,
    retry_in: f32,
    note: f32,
    version: u64,
    provisional: bool,
    joined: bool,
    approach: bool,
}

struct Roads {
    anchor: DVec2,
    lanes_seen: usize,
    verts: usize,
    built_at: f32,
}

pub struct Navigator {
    pub panel_overlay: Option<usize>,
    pub cockpit_display: bool,
    drawn_at: f32,
    pub enabled: bool,
    pub schedule: bool,
    speed_avg: f32,
    /// How far ahead of the bus the dimmed route begins (m), and when that was last moved.
    dim_ahead: f64,
    dim_at: f32,
    pub opacity: f32,
    pub corner: String,
    pub city: CityMap,
    panel_rect: [f32; 4],
    gpu: Option<Gpu>,
    fonts: Fonts,
    atlas: Atlas,
    target: Option<(TextureId, u32, u32)>,
    bottom_t: f32,
    bottom_e: f32,
    turn_t: f32,
    turn_e: f32,
    turn_shown: Option<(i32, f32, f64, Option<String>)>,
    pub show_topbar: bool,
    pub show_turn: bool,
    pub show_stoplist: bool,
    sched_t: f32,
    sched_e: f32,
    sched_rows: f32,
    own_net: Option<std::sync::Arc<Network>>,
    global: Option<std::sync::Arc<Network>>,
    stop_pos: std::sync::Arc<HashMap<i64, DVec3>>,
    streets: Option<std::sync::Arc<Streets>>,
    /// The streets drawn, built with the map's network.
    graph: Option<std::sync::Arc<RoadGraph>>,
    /// Navigator 2.0's ground: the surfaces the map really draws.
    surfaces: Option<std::sync::Arc<crate::navmap::SurfaceMap>>,
    #[allow(clippy::type_complexity)]
    building: Option<
        std::sync::mpsc::Receiver<(
            Network,
            HashMap<i64, DVec3>,
            Streets,
            RoadGraph,
            crate::navmap::SurfaceMap,
        )>,
    >,
    pub global_version: u64,
    roads: Option<Roads>,
    route: Route,
    route_mesh: RouteMesh,
    /// The route's lanes (their fingerprint) and how far along the route each begins.
    route_cum: (u64, Vec<f64>),
    congestion: HashMap<usize, f32>,
    route_jam: HashMap<usize, f32>,
    jam_version: u64,
    congestion_t: f32,
    zoom: f64,
    cam_heading: f64,
    time: f32,
    next_dist: Option<f64>,
    dist_t: f32,
    pub arrows: bool,
    pub show_ai: bool,
    shown: f32,
    stop_spots: Vec<(DVec3, String, f64, i64)>,
    bus_at: DVec3,
    next_turn: Option<(i32, f32, f64, Option<String>)>,
    street_here: Option<String>,
    jam_cost: f32,
    first: bool,
}

#[allow(clippy::type_complexity)]
pub fn duty_parts(
    duty: Option<&crate::schedule::PlayerDuty>,
) -> (
    Option<String>,
    Option<String>,
    Vec<NavStop>,
    Option<(String, String)>,
) {
    let Some(d) = duty else {
        return (None, None, Vec::new(), None);
    };
    let Some(trip) = d.trips.get(d.trip_index) else {
        return (None, None, Vec::new(), None);
    };
    let line = if trip.line.trim().is_empty() {
        d.line.trim()
    } else {
        trip.line.trim()
    };
    let stops = trip
        .stops
        .iter()
        .skip(d.next_stop)
        .filter(|s| s.stops)
        .map(|s| NavStop {
            object_id: s.object_id,
            position: s.position.unwrap_or(DVec3::ZERO),
            name: s.name.clone(),
            arrival: s.arr,
        })
        .collect();
    (
        Some(line.to_string()),
        Some(trip.terminus.clone()),
        stops,
        Some((format!("{}/{}", d.trip_index, trip.name), trip.name.clone())),
    )
}

impl<'a> NavFrame<'a> {
    pub(crate) fn clone_ref(&self) -> NavFrame<'a> {
        NavFrame {
            traffic: self.traffic,
            bus: self.bus,
            heading: self.heading,
            speed_kmh: self.speed_kmh,
            outside_temp: self.outside_temp,
            inside_temp: self.inside_temp,
            line: self.line.clone(),
            terminus: self.terminus.clone(),
            stops: self.stops.clone(),
            delay: self.delay,
            passengers: self.passengers,
            time: self.time,
            weekday: self.weekday,
            language: self.language,
            units: self.units,
            screen: self.screen,
            ui_scale: self.ui_scale,
            follow_window: self.follow_window,
            dt: self.dt,
            stop_requested: self.stop_requested,
        }
    }
}

#[derive(Debug)]
pub(crate) struct MapRoad {
    pub(crate) points: Vec<DVec3>,
    pub(crate) width: f32,
    pub(crate) main: bool,
    /// The spline (tile, id) the road runs along, if it is one.
    pub(crate) spline: Option<((i32, i32), i64)>,
}

/// What the route line in the panel was built for: the lanes (their fingerprint), the
/// traffic on them, where it is drawn from, its width, the lane it starts at and how far along
/// the route it reaches; `verts` vertices.
#[derive(Default)]
struct RouteMesh {
    lanes: u64,
    jam: u64,
    anchor: DVec2,
    px: f32,
    from: usize,
    end: f64,
    verts: u32,
}

/// A fingerprint of a route's lanes: a new way back changes it, driving along does not.
fn lanes_print(lanes: &[usize]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    lanes.hash(&mut h);
    h.finish() | 1
}

pub struct Streets {
    names: Vec<String>,
    of_lane: Vec<u32>,
    labels: Vec<(DVec2, f32, u32)>,
}

#[derive(Default)]
pub struct CityMap {
    pub open: bool,
    pub rect: [f32; 4],
    pub embed: Option<[f32; 4]>,
    pub picture: Option<TextureId>,
    center: DVec2,
    mpp: f64,
    follow: bool,
    drag: Option<(f32, f32)>,
    target: Option<(TextureId, u32, u32)>,
    roads: Option<(u64, u32, DVec2)>,
    /// The surfaces drawn: around where, how far, and whether coarsely.
    surf: Option<(DVec2, f64, bool)>,
    route: ((u64, u64, u32, u64), u32),
    extent: (DVec2, DVec2),
    buttons: Vec<(Rect, u8)>,
    /// Where the next drawing goes instead of `target` (`Navigator::city_shot`).
    shot: Option<wgpu::TextureView>,
}
