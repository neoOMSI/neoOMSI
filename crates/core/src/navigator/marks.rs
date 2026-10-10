//! What stands on the map: other vehicles as little top-down bodies, stops as pins and
//! the names beside them, text kept legible by a soft dark halo.

use super::*;

/// The dark ring round a label's letters, matching the map's ground.
pub(super) const HALO: Color = Color::rgba(14, 14, 16, 0.62);
/// A car of the traffic: quiet, it is only context.
pub(super) const CAR: Color = Color::rgba(150, 157, 175, 1.0);
/// A bus of the traffic: a colour of its own, so the other lines read at a glance.
pub(super) const AI_BUS: Color = Color::rgba(92, 148, 255, 1.0);
/// The dark rim round a vehicle.
pub(super) const RIM: Color = Color::rgba(10, 10, 12, 0.8);
/// A stop the route passes, before the next one.
pub(super) const STOP_DOT: Color = Color::rgba(242, 242, 244, 1.0);

/// A vehicle seen from above, as an outline (x right, y forward; half its length is 1):
/// a box with the corners taken off, the nose rounder than the tail.
fn body(half_w: f32) -> [Vec2; 8] {
    let w = half_w;
    [
        Vec2::new(-w * 0.72, 1.0),
        Vec2::new(w * 0.72, 1.0),
        Vec2::new(w, 0.86),
        Vec2::new(w, -0.93),
        Vec2::new(w * 0.86, -1.0),
        Vec2::new(-w * 0.86, -1.0),
        Vec2::new(-w, -0.93),
        Vec2::new(-w, 0.86),
    ]
}

/// The traffic around: cars as small bodies, buses longer and blue with a windscreen. Each
/// is its true size up close and never smaller than a few pixels; `keep` drops what is out
/// of sight before anything is built.
pub(super) fn vehicles(
    p: &mut Painter,
    t: &Traffic,
    rel: impl Fn(DVec3) -> Vec3,
    keep: impl Fn(DVec3) -> bool,
    cars: bool,
) {
    // buses last: they sit on top of the cars queuing beside them
    for buses in [false, true] {
        if !buses && !cars {
            continue;
        }
        for c in t.cars.iter().filter(|c| !c.gone && c.is_bus() == buses) {
            let at = c.vehicle.position;
            if !keep(at) {
                continue;
            }
            let half_l = c
                .vehicle
                .ty
                .half_length()
                .filter(|l| *l > 1.0)
                .unwrap_or(if buses { 6.0 } else { 2.2 });
            let half_w = if c.half_width > 0.3 {
                c.half_width
            } else if buses {
                1.25
            } else {
                0.9
            };
            let h = (c.vehicle.heading as f32).to_radians();
            // heading clockwise from north: forward (sin, cos), right (cos, -sin)
            let (sn, cs) = h.sin_cos();
            let turn = |v: Vec2| Vec2::new(v.x * cs + v.y * sn, -v.x * sn + v.y * cs);
            // a narrow body stays a body when drawn at its pixel minimum
            let shape = body((half_w / half_l).clamp(0.2, 0.46));
            let world = shape.map(turn);
            // a hairline rim, so a car keeps its own outline on a grey road
            let rim = shape.map(|v| turn(v + v.signum() * Vec2::new(0.09, 0.05)));
            // near its true size from afar too: blown up, a car is wider than its lane
            let (min_px, fill) = if buses { (5.0, AI_BUS) } else { (2.8, CAR) };
            let q = rel(at);
            p.world_shape(q, &rim, half_l, min_px, RIM);
            p.world_shape(q, &world, half_l, min_px, fill);
            // the windows say which way it faces: a bus a full-width windscreen, a car a
            // windscreen and a smaller rear window round its roof
            let w = (half_w / half_l).clamp(0.2, 0.46) * 0.78;
            let glass = |y0: f32, y1: f32, k: f32| {
                [
                    Vec2::new(-w * k, y0),
                    Vec2::new(w * k, y0),
                    Vec2::new(w * k, y1),
                    Vec2::new(-w * k, y1),
                ]
                .map(turn)
            };
            if buses {
                let tint = Color::rgba(22, 34, 58, 0.95);
                p.world_shape(q, &glass(0.93, 0.78, 1.0), half_l, min_px, tint);
            } else {
                let tint = Color::rgba(44, 48, 60, 0.9);
                p.world_shape(q, &glass(0.5, 0.18, 1.0), half_l, min_px, tint);
                p.world_shape(q, &glass(-0.55, -0.72, 0.9), half_l, min_px, tint);
            }
        }
    }
}

/// Text with its baseline at `at` and a soft halo round it, pixel-snapped so it stays
/// crisp; returns its width.
#[allow(clippy::too_many_arguments)]
pub(super) fn halo_text(
    ui: &mut Painter,
    atlas: &mut Atlas,
    fonts: &Fonts,
    text: &str,
    px: f32,
    weight: Weight,
    at: Vec2,
    align: Align,
    c: Color,
    s: f32,
) -> f32 {
    let d = 1.1 * s.max(1.0);
    for o in [
        Vec2::new(d, 0.0),
        Vec2::new(-d, 0.0),
        Vec2::new(0.0, d),
        Vec2::new(0.0, -d),
        Vec2::new(0.7, 0.7) * d,
        Vec2::new(-0.7, 0.7) * d,
        Vec2::new(0.7, -0.7) * d,
        Vec2::new(-0.7, -0.7) * d,
    ] {
        ui.text(
            atlas,
            fonts,
            text,
            px,
            weight,
            at + o,
            align,
            HALO.alpha(c.0[3]),
        );
    }
    ui.text(atlas, fonts, text, px, weight, at, align, c)
}

/// Rotated text (a street name along its street) with a halo.
#[allow(clippy::too_many_arguments)]
pub(super) fn halo_text_rotated(
    ui: &mut Painter,
    atlas: &mut Atlas,
    fonts: &Fonts,
    text: &str,
    px: f32,
    weight: Weight,
    at: Vec2,
    angle: f32,
    c: Color,
    s: f32,
) {
    let d = 1.1 * s.max(1.0);
    for o in [
        Vec2::new(d, 0.0),
        Vec2::new(-d, 0.0),
        Vec2::new(0.0, d),
        Vec2::new(0.0, -d),
        Vec2::new(0.7, 0.7) * d,
        Vec2::new(-0.7, 0.7) * d,
        Vec2::new(0.7, -0.7) * d,
        Vec2::new(-0.7, -0.7) * d,
    ] {
        ui.text_rotated(
            atlas,
            fonts,
            text,
            px,
            weight,
            at + o,
            angle,
            HALO.alpha(c.0[3]),
        );
    }
    ui.text_rotated(atlas, fonts, text, px, weight, at, angle, c);
}

/// Which stop a pin stands for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Pin {
    Next,
    On,
    Last,
}

impl Pin {
    pub(super) fn of(k: usize, n: usize) -> Pin {
        if k == 0 {
            Pin::Next
        } else if k + 1 == n {
            Pin::Last
        } else {
            Pin::On
        }
    }
}

/// A stop's pin at `p`: the next one an amber badge with a bus and a soft glow, the last a
/// white badge with the chequered flag, the ones between small white dots on the route.
pub(super) fn stop_pin(ui: &mut Painter, atlas: &mut Atlas, p: Vec2, pin: Pin, s: f32) {
    match pin {
        Pin::Next => {
            ui.circle(p, 13.0 * s, ACCENT.alpha(0.16));
            ui.circle(p, 9.0 * s, RIM);
            ui.circle(p, 7.6 * s, ACCENT);
            ui.icon(
                atlas,
                "directions_bus",
                p,
                11.0 * s,
                Color::rgba(20, 14, 6, 1.0),
            );
        }
        Pin::Last => {
            ui.circle(p, 8.6 * s, RIM);
            ui.circle(p, 7.2 * s, STOP_DOT);
            ui.icon(
                atlas,
                "sports_score",
                p,
                10.5 * s,
                Color::rgba(18, 18, 20, 1.0),
            );
        }
        Pin::On => {
            ui.circle(p, 5.0 * s, RIM);
            ui.circle(p, 3.6 * s, STOP_DOT);
            ui.circle(p, 1.5 * s, ACCENT.darken(0.15));
        }
    }
}

/// How much room a stop's label takes: `(width, name, time)`, the name cut to fit.
pub(super) fn stop_label_size(
    fonts: &Fonts,
    name: &str,
    time: Option<&str>,
    pin: Pin,
    max: f32,
    s: f32,
) -> (f32, String, f32) {
    let (px, weight) = label_font(pin, s);
    let tw = time
        .map(|t| fonts.width(t, px * 0.92, Weight::Medium) + 6.0 * s)
        .unwrap_or(0.0);
    let name = fonts.fit(name, px, weight, (max - tw).max(40.0 * s));
    let nw = fonts.width(&name, px, weight);
    let pad = if pin == Pin::On { 0.0 } else { 9.0 * s };
    (nw + tw + 2.0 * pad, name, nw)
}

fn label_font(pin: Pin, s: f32) -> (f32, Weight) {
    match pin {
        Pin::Next => (12.5 * s, Weight::Bold),
        Pin::Last => (12.0 * s, Weight::Bold),
        Pin::On => (11.5 * s, Weight::Medium),
    }
}

/// A stop's label in `r`: the next and the last stop on a card with a soft shadow, the
/// ones between as bare text with a halo - the map shows through, nothing boxes it in.
#[allow(clippy::too_many_arguments)]
pub(super) fn stop_label(
    ui: &mut Painter,
    atlas: &mut Atlas,
    fonts: &Fonts,
    r: Rect,
    name: &str,
    name_w: f32,
    time: Option<&str>,
    pin: Pin,
    s: f32,
) {
    let (px, weight) = label_font(pin, s);
    let base = r.center().y + fonts.cap_height(px, weight) * 0.5;
    if pin == Pin::On {
        halo_text(
            ui,
            atlas,
            fonts,
            name,
            px,
            weight,
            Vec2::new(r.x, base),
            Align::Left,
            Color::rgba(226, 226, 230, 1.0),
            s,
        );
        if let Some(t) = time {
            halo_text(
                ui,
                atlas,
                fonts,
                t,
                px * 0.92,
                Weight::Medium,
                Vec2::new(r.x + name_w + 6.0 * s, base),
                Align::Left,
                TEXT_DIM.alpha(0.9),
                s,
            );
        }
        return;
    }
    let radius = r.h * 0.5;
    ui.shadow(
        Rect::new(r.x, r.y + 2.0 * s, r.w, r.h),
        radius,
        10.0 * s,
        Color::rgba(0, 0, 0, 0.45),
    );
    ui.rounded(r, radius, Color::rgba(26, 26, 29, 0.97));
    ui.rounded_border(
        r,
        radius,
        1.0_f32.max(s),
        if pin == Pin::Next {
            ACCENT.alpha(0.45)
        } else {
            Color::WHITE.alpha(0.08)
        },
    );
    let x = r.x + 9.0 * s;
    ui.text(
        atlas,
        fonts,
        name,
        px,
        weight,
        Vec2::new(x, base),
        Align::Left,
        TEXT,
    );
    if let Some(t) = time {
        ui.text(
            atlas,
            fonts,
            t,
            px * 0.92,
            Weight::Medium,
            Vec2::new(x + name_w + 6.0 * s, base),
            Align::Left,
            if pin == Pin::Next { ACCENT } else { TEXT_DIM },
        );
    }
}

/// The player's bus: an arrow pointing `angle` (radians, clockwise on screen) on a soft
/// shadow, with a glow of `glow` round it when given.
pub(super) fn own_arrow(
    ui: &mut Painter,
    at: Vec2,
    angle: f32,
    size: f32,
    fill: Color,
    glow: Option<Color>,
) {
    let (sn, cs) = angle.sin_cos();
    let rot = |v: Vec2| at + Vec2::new(v.x * cs - v.y * sn, v.x * sn + v.y * cs) * size;
    let (tip, l, m, r) = (
        rot(Vec2::new(0.0, -1.0)),
        rot(Vec2::new(-0.72, 0.82)),
        rot(Vec2::new(0.0, 0.38)),
        rot(Vec2::new(0.72, 0.82)),
    );
    match glow {
        Some(g) => ui.circle(at, size * 1.45, g.alpha(0.18)),
        None => ui.circle(at, size * 1.5, Color::rgba(0, 0, 0, 0.35)),
    }
    let grow = |p: Vec2| at + (p - at) * 1.32;
    ui.tri(grow(tip), grow(l), grow(m), RIM, RIM, RIM);
    ui.tri(grow(tip), grow(m), grow(r), RIM, RIM, RIM);
    let shade = fill.darken(0.14);
    ui.tri(tip, l, m, fill, fill, fill);
    ui.tri(tip, m, r, shade, shade, shade);
}
