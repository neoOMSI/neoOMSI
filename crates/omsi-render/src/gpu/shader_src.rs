pub(crate) fn scene_shader_source(gl: bool) -> String {
    let src = [
        include_str!("../../shaders/scene/scene_base.wgsl"),
        include_str!("../../shaders/enhanced/common_lighting.wgsl"),
        include_str!("../../shaders/puddles/puddle_common.wgsl"),
        include_str!("../../shaders/enhanced/scene_lighting.wgsl"),
    ]
    .join("\n");
    if !gl {
        return src;
    }
    let clamped = |t: &str| {
        format!(
            "textureSample({t}, s_diffuse, clamp(uv, 0.5 / vec2<f32>(textureDimensions({t})), \
             vec2<f32>(1.0) - 0.5 / vec2<f32>(textureDimensions({t}))))"
        )
    };
    let out = src
        .replace("textureSample(t_trans, s_tile, uv)", &clamped("t_trans"))
        .replace("textureSample(t_night, s_tile, uv)", &clamped("t_night"));
    debug_assert!(!out.contains("s_tile, uv)"));
    out
}

pub(crate) fn sky_shader_source() -> String {
    [
        include_str!("../../shaders/sky/sky_base.wgsl"),
        include_str!("../../shaders/enhanced/common_lighting.wgsl"),
        include_str!("../../shaders/enhanced/sky_lighting.wgsl"),
    ]
    .join("\n")
}

pub(crate) fn corona_shader_source() -> String {
    [
        include_str!("../../shaders/sky/corona.wgsl"),
        include_str!("../../shaders/enhanced/common_lighting.wgsl"),
    ]
    .join("\n")
}
