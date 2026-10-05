use crate::*;

pub(crate) fn debug_view() -> f32 {
    #[cfg(all(feature = "devtools", debug_assertions))]
    if let Some(v) = devtools::debug_view_override() {
        return v as f32;
    }
    static VIEW: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *VIEW.get_or_init(|| {
        omsi_cfg::env::var("OMSI_DEBUG_ENHANCED")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0)
    })
}

pub(crate) fn night_scale(night: f32, brightness: f32) -> f32 {
    1.0 + (brightness.clamp(0.0, 4.0) - 1.0) * night.clamp(0.0, 1.0)
}

pub(crate) fn meter_tuning() -> [f32; 6] {
    static METER: std::sync::OnceLock<[f32; 6]> = std::sync::OnceLock::new();
    *METER.get_or_init(|| {
        let mut m = [
            METER_GAIN,
            METER_TARGET,
            METER_DARKEN,
            METER_BRIGHTEN,
            0.0,
            NIGHT_VISION,
        ];
        if let Ok(v) = omsi_cfg::env::var("OMSI_METER") {
            for (k, x) in v.split(',').take(6).enumerate() {
                if let Ok(x) = x.trim().parse() {
                    m[k] = x;
                }
            }
        }
        m
    })
}

pub(crate) fn sky_input_differs(a: &atmosphere::SkyInput, b: &atmosphere::SkyInput) -> bool {
    let near = |x: f32, y: f32, tol: f32| (x - y).abs() <= tol;
    a.sun_dir.dot(b.sun_dir) < 0.999_998
        || !near(a.sun_visibility, b.sun_visibility, 0.01)
        || !near(a.overcast, b.overcast, 0.01)
        || !near(a.haze, b.haze, 0.02)
        || !near(a.rain, b.rain, 0.01)
        || !near(a.ground_albedo, b.ground_albedo, 0.01)
        || !near(a.night_light, b.night_light, 0.01)
        || a.tint
            .iter()
            .zip(&b.tint)
            .any(|(x, y)| (*x - *y).abs().max_element() > 0.01)
}

pub(crate) fn half_to_f32(b: u16) -> f32 {
    let sign = if b & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((b >> 10) & 0x1f) as i32;
    let m = (b & 0x3ff) as f32;
    match e {
        0 => sign * m / 1024.0 * 2f32.powi(-14),
        31 => sign * f32::INFINITY,
        _ => sign * (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

pub(crate) fn origin_key(origin: DVec3) -> [u64; 3] {
    origin
        .to_array()
        .map(|v| if v == 0.0 { 0 } else { v.to_bits() })
}

pub(crate) fn nearest_by_origin(
    items: impl IntoIterator<Item = (DVec3, f32)>,
) -> HashMap<[u64; 3], f32> {
    let mut out: HashMap<[u64; 3], f32> = HashMap::new();
    for (origin, d) in items {
        if origin.is_nan() {
            continue;
        }
        out.entry(origin_key(origin))
            .and_modify(|best| *best = best.min(d))
            .or_insert(d);
    }
    out
}

pub(crate) fn in_scope<'s, R>(
    pool: Option<&rayon::ThreadPool>,
    op: impl FnOnce(&rayon::Scope<'s>) -> R,
) -> R {
    match pool {
        Some(p) => p.in_place_scope(op),
        None => rayon::in_place_scope(op),
    }
}

pub(crate) fn split_parts(pool: Option<&rayon::ThreadPool>, n: usize) -> (usize, usize) {
    let parts = (n / 8192).clamp(1, pool.map_or(3, |p| p.current_num_threads()) + 1);
    (parts, n.div_ceil(parts))
}

pub(crate) fn run_parts<T: Send>(
    pool: Option<&rayon::ThreadPool>,
    parts: usize,
    f: impl Fn(usize) -> T + Sync,
) -> Vec<T> {
    let Some(pool) = pool else {
        return std::thread::scope(|s| {
            let f = &f;
            let helpers: Vec<_> = (1..parts.max(1)).map(|k| s.spawn(move || f(k))).collect();
            let mut out = vec![f(0)];
            out.extend(helpers.into_iter().map(|h| h.join().expect("render part")));
            out
        });
    };
    let mut out: Vec<Option<T>> = (0..parts.max(1)).map(|_| None).collect();
    pool.in_place_scope(|s| {
        let f = &f;
        let (first, rest) = out.split_first_mut().expect("one part at least");
        for (k, slot) in rest.iter_mut().enumerate() {
            s.spawn(move |_| *slot = Some(f(k + 1)));
        }
        *first = Some(f(0));
    });
    out.into_iter().map(|o| o.expect("render part")).collect()
}

pub(crate) fn point_in_vehicle_box(
    p: DVec3,
    (origin, heading, bb): &(DVec3, f64, [f32; 6]),
) -> bool {
    let d = (p - *origin).as_vec3();
    let (sh, ch) = (*heading as f32).to_radians().sin_cos();
    let x = d.x * ch - d.y * sh - bb[3];
    let y = d.x * sh + d.y * ch - bb[4];
    let z = d.z - bb[5];
    x.abs() < bb[0] * 0.5 && y.abs() < bb[1] * 0.5 && z.abs() < bb[2] * 0.5
}

pub(crate) fn snap_rect(r: [f32; 4]) -> [f32; 4] {
    let snap = |v: f32| (v + 0.5).floor();
    let (x0, y0) = (snap(r[0]), snap(r[1]));
    let x1 = if r[2] > r[0] {
        snap(r[2]).max(x0 + 1.0)
    } else {
        snap(r[2])
    };
    let y1 = if r[3] > r[1] {
        snap(r[3]).max(y0 + 1.0)
    } else {
        snap(r[3])
    };
    [x0, y0, x1, y1]
}
