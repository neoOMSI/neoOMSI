use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

static WIREFRAME: AtomicBool = AtomicBool::new(false);
static WIREFRAME_OK: AtomicBool = AtomicBool::new(false);
static VIEW: AtomicI32 = AtomicI32::new(-1);

pub const DEBUG_VIEWS: &[(i32, &str)] = &[
    (0, "Normal"),
    (1, "Sun shadow"),
    (2, "AO"),
    (3, "Normals"),
    (4, "Air transmittance"),
    (5, "Ambient light"),
    (6, "Reflection"),
    (7, "Albedo"),
    (8, "Direct sun"),
    (9, "In-scattered air"),
    (10, "Alpha / Glas / Envmap"),
    (11, "Distance / depth"),
    (12, "Alpha mode / terrain / surface"),
    (13, "Cab / AO / specular occlusion"),
    (14, "Direct + ambient"),
    (15, "Lamps and headlights"),
    (16, "Self-illuminated"),
    (17, "Roughness / F0 / metalness"),
];

pub fn set_wireframe(on: bool) {
    WIREFRAME.store(on, Ordering::Relaxed);
}

pub fn wireframe() -> bool {
    WIREFRAME.load(Ordering::Relaxed) && WIREFRAME_OK.load(Ordering::Relaxed)
}

pub fn wireframe_supported() -> bool {
    WIREFRAME_OK.load(Ordering::Relaxed)
}

pub(crate) fn set_wireframe_supported(ok: bool) {
    WIREFRAME_OK.store(ok, Ordering::Relaxed);
}

pub fn set_debug_view(view: Option<i32>) {
    VIEW.store(view.unwrap_or(-1), Ordering::Relaxed);
}

pub fn debug_view_override() -> Option<i32> {
    let v = VIEW.load(Ordering::Relaxed);
    (v >= 0).then_some(v)
}