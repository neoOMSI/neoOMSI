pub(super) fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

pub(super) fn hms(t: f64) -> String {
    let t = t.max(0.0) as u64;
    format!("{:02}:{:02}:{:02}", (t / 3600) % 24, (t / 60) % 60, t % 60)
}

pub(crate) fn project(
    vp: &glam::Mat4,
    cam: glam::DVec3,
    p: glam::DVec3,
    size: (u32, u32),
) -> Option<[f32; 2]> {
    let c = vp.mul_vec4((p - cam).as_vec3().extend(1.0));
    if c.w <= 0.01 {
        return None;
    }
    let (x, y) = (c.x / c.w, c.y / c.w);
    Some([
        (x * 0.5 + 0.5) * size.0 as f32,
        (0.5 - y * 0.5) * size.1 as f32,
    ])
}
