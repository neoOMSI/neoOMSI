use super::*;

pub(super) const NAV_REDRAW_S: f32 = 1.0 / 30.0;
pub(super) const PANEL: Color = Color::rgba(18, 18, 20, 1.0);
pub(super) const CARD: Color = Color::rgba(24, 24, 26, 1.0);
pub(super) const HAIR: Color = Color::rgba(40, 40, 44, 1.0);
pub(super) const ACCENT: Color = Color::rgba(232, 160, 48, 1.0);
pub(super) const BAR: Color = Color::rgba(18, 18, 20, 1.0);
pub(super) const ROAD_CASING: Color = Color::rgba(18, 18, 20, 0.9);
pub(super) const ROAD: Color = Color::rgba(56, 56, 63, 1.0);
pub(super) const ROAD_MAIN: Color = Color::rgba(78, 78, 88, 1.0);
/// The pavement edge along a carriageway of the road graph.
pub(super) const ROAD_KERB: Color = Color::rgba(34, 34, 39, 1.0);
pub(super) const ROUTE: Color = Color::rgba(214, 48, 40, 1.0);
pub(super) const DOT: Color = Color::rgba(70, 140, 255, 1.0);

pub(super) const LEVEL: [Color; 5] = [
    Color::rgba(232, 160, 48, 1.0),
    Color::rgba(56, 178, 86, 1.0),
    Color::rgba(246, 228, 96, 1.0),
    Color::rgba(224, 56, 44, 1.0),
    Color::rgba(122, 16, 22, 1.0),
];

/// The dark edge along the route line.
pub(super) const ROUTE_EDGE: Color = Color::rgba(14, 12, 10, 0.95);
/// How far ahead of the bus the route shows in the panel (m).
pub(super) const ROUTE_AHEAD: f64 = 2500.0;
/// The route line's width in the panel (px at the panel's own scale).
pub(super) const ROUTE_PX: f32 = 7.0;

pub(super) const DRIVEN: Color = Color::rgba(30, 30, 34, 1.0);
pub(super) const STREET: Color = Color::rgba(190, 190, 196, 1.0);

pub(super) fn level(score: f32) -> usize {
    match score {
        s if s < 0.12 => 0,
        s if s < 0.40 => 1,
        s if s < 0.60 => 2,
        s if s < 0.80 => 3,
        _ => 4,
    }
}

pub(super) const TEXT: Color = Color::rgba(235, 235, 235, 1.0);
pub(super) const TEXT_DIM: Color = Color::rgba(178, 178, 178, 1.0);
pub(super) const LATE: Color = Color::rgba(235, 85, 70, 1.0);
pub(super) const EARLY: Color = Color::rgba(90, 160, 240, 1.0);
pub(super) const ON_TIME: Color = Color::rgba(110, 200, 120, 1.0);
pub(super) const WARN: Color = Color::rgba(235, 170, 60, 1.0);
pub(super) const STOP_REQUEST: Color = Color::hex(0xF0A030);
pub(super) const FOV: f32 = 40.0;
pub(super) const PITCH: f64 = 52.0;
pub(super) const ROAD_RADIUS: f64 = 1300.0;
pub(super) const OFF_ROUTE_AFTER: f32 = 2.0;
pub(super) const REROUTE_EVERY: f32 = 2.5;

