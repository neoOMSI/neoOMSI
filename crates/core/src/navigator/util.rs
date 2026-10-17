use super::*;

pub(super) fn project(vp: Mat4, viewport: [f32; 4], p: Vec3) -> Option<Vec2> {
    let c = vp * p.extend(1.0);
    if c.w <= 0.1 {
        return None;
    }
    let n = c.truncate() / c.w;
    Some(Vec2::new(
        viewport[0] + (n.x * 0.5 + 0.5) * viewport[2],
        viewport[1] + (0.5 - n.y * 0.5) * viewport[3],
    ))
}

pub(super) fn angle_diff(a: f64, b: f64) -> f64 {
    let mut d = (b - a) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    d
}

pub(super) fn ease(dt: f32, tau: f32) -> f64 {
    (1.0 - (-dt / tau.max(1e-3)).exp()) as f64
}

pub(super) fn map_samples(format: wgpu::TextureFormat) -> u32 {
    if format
        .guaranteed_format_features(wgpu::Features::empty())
        .flags
        .sample_count_supported(4)
    {
        4
    } else {
        1
    }
}

pub(super) fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

pub(super) fn stop_label_rect(p: Vec2, width: f32, s: f32, win: Rect, taken: &[Rect]) -> Option<Rect> {
    let h = 20.0 * s;
    let gap = 10.0 * s;
    let positions = [
        Vec2::new(p.x + gap, p.y - h * 0.5),
        Vec2::new(p.x - gap - width, p.y - h * 0.5),
        Vec2::new(p.x + gap, p.y - h - gap),
        Vec2::new(p.x + gap, p.y + gap),
        Vec2::new(p.x - gap - width, p.y - h - gap),
        Vec2::new(p.x - gap - width, p.y + gap),
    ];
    positions
        .into_iter()
        .map(|q| Rect::new(q.x, q.y, width, h))
        .find(|r| {
            r.x >= 8.0 * s
                && r.right() <= win.right() - 8.0 * s
                && r.y >= 50.0 * s
                && r.bottom() <= win.bottom() - 8.0 * s
                && !taken.iter().any(|t| rects_overlap(t, r))
        })
}

pub(super) fn spaced_markers(points: impl IntoIterator<Item=(usize, Vec2)>, gap: f32) -> Vec<(usize, Vec2)> {
    let mut kept: Vec<(usize, Vec2)> = Vec::new();
    for (k, p) in points {
        if kept.iter().all(|(_, q)| p.distance(*q) >= gap) {
            kept.push((k, p));
        }
    }
    kept
}

#[allow(dead_code)]
pub(super) fn heading_vec(h: f64) -> DVec2 {
    let m = DMat3::from_rotation_z(-h.to_radians());
    m.transform_vector2(DVec2::Y)
}

/// A time of day (s since midnight) as `HH:MM`.
pub(super) fn clock(secs: f64) -> String {
    format!(
        "{:02}:{:02}",
        (secs / 3600.0) as i32 % 24,
        ((secs % 3600.0) / 60.0) as i32
    )
}

pub(super) fn uses_miles(units: &str) -> bool {
    units.eq_ignore_ascii_case("uk") || units.eq_ignore_ascii_case("imperial")
}

pub(super) fn speed(kmh: f32, miles: bool) -> f32 {
    if miles { kmh * 0.621_371 } else { kmh }
}

pub(super) fn distance(metres: f64, miles: bool) -> String {
    if miles {
        if metres >= 1609.344 {
            format!("{:.1} mi", metres / 1609.344)
        } else {
            format!("{:.0} yd", metres * 1.093_613_3)
        }
    } else if metres >= 1000.0 {
        format!("{:.1} km", metres / 1000.0)
    } else {
        format!("{metres:.0} m")
    }
}

pub(super) fn rounded_distance(metres: f64, miles: bool, minimum: f64) -> String {
    if miles {
        if metres >= 1609.344 {
            format!("{:.1} mi", metres / 1609.344)
        } else {
            format!(
                "{:.0} yd",
                ((metres * 1.093_613_3 / 10.0).round() * 10.0).max(minimum)
            )
        }
    } else if metres >= 1000.0 {
        format!("{:.1} km", metres / 1000.0)
    } else {
        format!("{:.0} m", ((metres / 10.0).round() * 10.0).max(minimum))
    }
}

