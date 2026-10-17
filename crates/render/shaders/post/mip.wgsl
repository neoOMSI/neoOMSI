// Mipmap generation: alpha-weight RGB so bright RGB stored in transparent texels does not
// bleed into tree/fence silhouettes when the texture switches to a smaller mip. Where all
// four texels have alpha 0 the plain average is kept: on opaque materials the alpha channel
// is often only the envmap mask, and 0 there (lamps, wheels) must not turn the colour black.
@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var s_src: sampler;
struct VsOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    var out: VsOut;
    out.clip = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}
struct Footprint { rgb: vec3<f32>, plain: vec3<f32>, alpha: f32, total: f32 };

fn footprint(clip: vec2<f32>) -> Footprint {
    let dim = vec2<i32>(textureDimensions(t_src));
    let src = vec2<f32>(dim);
    let dst = max(floor(src * 0.5), vec2<f32>(1.0));
    let px = floor(clip);
    let lo = px * src / dst;
    let hi = (px + vec2<f32>(1.0)) * src / dst;
    let first = vec2<i32>(floor(lo));
    let last = vec2<i32>(ceil(hi)) - vec2<i32>(1);
    var rgb = vec3<f32>(0.0);
    var plain = vec3<f32>(0.0);
    var alpha = 0.0;
    var total = 0.0;
    for (var y = first.y; y <= last.y; y = y + 1) {
        let wy = min(hi.y, f32(y + 1)) - max(lo.y, f32(y));
        for (var x = first.x; x <= last.x; x = x + 1) {
            let wx = min(hi.x, f32(x + 1)) - max(lo.x, f32(x));
            let w = wx * wy;
            let q = textureLoad(t_src, clamp(vec2<i32>(x, y), vec2<i32>(0), dim - vec2<i32>(1)), 0);
            rgb = rgb + q.rgb * (q.a * w);
            plain = plain + q.rgb * w;
            alpha = alpha + q.a * w;
            total = total + w;
        }
    }
    return Footprint(rgb, plain, alpha, total);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let f = footprint(in.clip.xy);
    return vec4<f32>(select(f.plain / f.total, f.rgb / f.alpha, f.alpha > 1e-5), f.alpha / f.total);
}

@fragment
fn fs_plain(in: VsOut) -> @location(0) vec4<f32> {
    let f = footprint(in.clip.xy);
    return vec4<f32>(f.plain / f.total, f.alpha / f.total);
}
