// The frosted backdrop of an overlay: the picture beneath it shrunk to a quarter, blurred
// across and down, and laid back under it with its rounded corners.
struct Params { a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, };
// shrink and blur: a.xy one texel of the input, a.zw the blur's direction (0 for the shrink)
// back under the overlay: a its rectangle in NDC (x0, y0, x1, y1), b where that lies in the
// blurred picture (u0, v0, u1, v1), c.xy its size and c.z its corner radius (pixels)
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var t_img: texture_2d<f32>;
@group(0) @binding(2) var s_img: sampler;

struct VsOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32>, };

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> VsOut {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    var out: VsOut;
    out.clip = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

// four bilinear taps around the centre: each output texel the mean of 4x4 input texels
@fragment
fn fs_shrink(in: VsOut) -> @location(0) vec4<f32> {
    let t = p.a.xy;
    let c = textureSample(t_img, s_img, in.uv + vec2<f32>(-t.x, -t.y))
        + textureSample(t_img, s_img, in.uv + vec2<f32>(t.x, -t.y))
        + textureSample(t_img, s_img, in.uv + vec2<f32>(-t.x, t.y))
        + textureSample(t_img, s_img, in.uv + vec2<f32>(t.x, t.y));
    return vec4<f32>(c.rgb * 0.25, 1.0);
}

// nine taps of a Gaussian (sigma 2 texels) along a.zw
@fragment
fn fs_blur(in: VsOut) -> @location(0) vec4<f32> {
    let w = array<f32, 5>(0.2042, 0.1802, 0.1238, 0.0663, 0.0276);
    let d = p.a.xy * p.a.zw;
    var c = textureSample(t_img, s_img, in.uv).rgb * w[0];
    for (var k = 1; k < 5; k++) {
        let o = d * f32(k);
        c += (textureSample(t_img, s_img, in.uv + o).rgb
            + textureSample(t_img, s_img, in.uv - o).rgb) * w[k];
    }
    return vec4<f32>(c, 1.0);
}

@vertex
fn vs_rect(@builtin(vertex_index) vid: u32) -> VsOut {
    let corners = array<vec2<f32>, 6>(vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0));
    let c = corners[vid];
    var out: VsOut;
    out.clip = vec4<f32>(mix(p.a.x, p.a.z, c.x), mix(p.a.y, p.a.w, c.y), 0.0, 1.0);
    out.uv = c;
    return out;
}

@fragment
fn fs_under(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(t_img, s_img, mix(p.b.xy, p.b.zw, in.uv)).rgb;
    // the rounded rectangle, antialiased over a pixel
    let half = p.c.xy * 0.5;
    let r = min(p.c.z, min(half.x, half.y));
    let q = abs(in.uv * p.c.xy - half) - (half - vec2<f32>(r));
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    let a = clamp(0.5 - d, 0.0, 1.0);
    return vec4<f32>(c * a, a);
}
