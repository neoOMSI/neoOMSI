// Enhanced graphics: what happens to the pre-exposed high-range picture before it reaches
// the screen - nothing that paints over it. A glow that only real highlights produce
// (a wide blur of the picture mixed in at a few per cent: a lamp a hundred times brighter
// than white spreads, a white wall does not), automatic exposure that meters the picture
// and follows it slowly within a narrow range, a neutral highlight shoulder and
// gentle luminance-only midtone lift, dithering against banding, FXAA.
// The vanilla path never runs this.
override OUTPUT_SRGB: bool = true;

struct PostParams {
    // x glow strength, y how far the metering may darken (EV), z brighten (EV),
    // w adaptation this frame (0..1; 1 = at once)
    a: vec4<f32>,
    // x how fast the exposure follows a darker picture, y a brighter one (fractions of the
    // way per frame), z the metering target (log2 of the mean luminance), w exposure bias (EV)
    b: vec4<f32>,
    // x how much of the metered difference is corrected (the light model's own exposure is
    // an incident meter: snow stays white and a night dark; the metering only helps where
    // it cannot know, a dark cab or an underpass), y night vision strength, z the scene's
    // pre-exposure (for absolute luminance), w how much an LED panel's dots count for in
    // the glow's source (0 = not at all, `Led glow`)
    c: vec4<f32>,
    d: vec4<f32>,
};
@group(0) @binding(0) var<uniform> p: PostParams;
@group(0) @binding(1) var t_src: texture_2d<f32>;
@group(0) @binding(2) var s_lin: sampler;
@group(0) @binding(3) var t_base: texture_2d<f32>;
@group(0) @binding(4) var t_adapt: texture_2d<f32>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    var out: VsOut;
    out.clip = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// A pixel that is not a finite number (a NaN or an infinity from the scene) as black -
// tested on the bits, which fast maths cannot drop. Unguarded, the glow's down- and
// up-sampling spread one such pixel over the picture as half and fully black squares
// (Vulkan and Direct3D keep NaNs, Metal's fast maths happened to hide them).
fn clean(c: vec3<f32>) -> vec3<f32> {
    let e = bitcast<vec3<u32>>(c) & vec3<u32>(0x7f800000u);
    let ok = all(e != vec3<u32>(0x7f800000u));
    return select(vec3<f32>(0.0), clamp(c, vec3<f32>(0.0), vec3<f32>(65000.0)), ok);
}

fn src(uv: vec2<f32>, texel: vec2<f32>, x: f32, y: f32) -> vec3<f32> {
    return clean(textureSampleLevel(t_src, s_lin, uv + vec2<f32>(x, y) * texel, 0.0).rgb);
}

// The picture without the bus's own screens (the screen mask is the first glow level's
// t_base): a lit display glows no halo over its own letters. An LED panel's dots are the
// panel's own light, though - they stay in the source (the mask's g) and count for several
// times their colour there (`p.c.w`), so that the faint mix the glow is blooms a halo
// around the panel without the dots themselves having to burn.
fn src_unmasked(uv: vec2<f32>, texel: vec2<f32>, x: f32, y: f32) -> vec3<f32> {
    let at = uv + vec2<f32>(x, y) * texel;
    let m = textureSampleLevel(t_base, s_lin, at, 0.0);
    let c = clean(textureSampleLevel(t_src, s_lin, at, 0.0).rgb);
    let screen = step(0.5, m.r);
    let led = step(0.9, m.g);
    let night = step(0.6, m.g) * (1.0 - led);
    let led_glow = min(c * (led * p.c.w), vec3<f32>(10.0));
    let night_glow = min(c * (night * p.d.x), vec3<f32>(10.0));
    return (c * (1.0 - screen) + led_glow * screen) * (1.0 - night) + night_glow;
}

// --- the glow: 13-tap downsampling (the first level with Karis' average, so that a single
// bright pixel does not flicker), tent upsampling

@fragment
fn fs_down_first(in: VsOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(t_src));
    let uv = in.uv;
    let a = src_unmasked(uv, texel, -2.0, -2.0);
    let b = src_unmasked(uv, texel, 0.0, -2.0);
    let c = src_unmasked(uv, texel, 2.0, -2.0);
    let d = src_unmasked(uv, texel, -2.0, 0.0);
    let e = src_unmasked(uv, texel, 0.0, 0.0);
    let f = src_unmasked(uv, texel, 2.0, 0.0);
    let g = src_unmasked(uv, texel, -2.0, 2.0);
    let h = src_unmasked(uv, texel, 0.0, 2.0);
    let i = src_unmasked(uv, texel, 2.0, 2.0);
    let j = src_unmasked(uv, texel, -1.0, -1.0);
    let k = src_unmasked(uv, texel, 1.0, -1.0);
    let l = src_unmasked(uv, texel, -1.0, 1.0);
    let m = src_unmasked(uv, texel, 1.0, 1.0);
    let g0 = (j + k + l + m) * 0.25;
    let g1 = (a + b + d + e) * 0.25;
    let g2 = (b + c + e + f) * 0.25;
    let g3 = (d + e + g + h) * 0.25;
    let g4 = (e + f + h + i) * 0.25;
    let w0 = 0.5 / (1.0 + luma(g0));
    let w1 = 0.125 / (1.0 + luma(g1));
    let w2 = 0.125 / (1.0 + luma(g2));
    let w3 = 0.125 / (1.0 + luma(g3));
    let w4 = 0.125 / (1.0 + luma(g4));
    let sum = g0 * w0 + g1 * w1 + g2 * w2 + g3 * w3 + g4 * w4;
    return vec4<f32>(sum / (w0 + w1 + w2 + w3 + w4), 1.0);
}

@fragment
fn fs_down(in: VsOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(t_src));
    let uv = in.uv;
    let outer = src(uv, texel, -2.0, -2.0) + src(uv, texel, 2.0, -2.0) + src(uv, texel, -2.0, 2.0) + src(uv, texel, 2.0, 2.0);
    let arms = src(uv, texel, 0.0, -2.0) + src(uv, texel, -2.0, 0.0) + src(uv, texel, 2.0, 0.0) + src(uv, texel, 0.0, 2.0);
    let inner = src(uv, texel, -1.0, -1.0) + src(uv, texel, 1.0, -1.0) + src(uv, texel, -1.0, 1.0) + src(uv, texel, 1.0, 1.0);
    let c = src(uv, texel, 0.0, 0.0) * 0.125 + outer * 0.03125 + arms * 0.0625 + inner * 0.125;
    return vec4<f32>(c, 1.0);
}

@fragment
fn fs_up(in: VsOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(t_src));
    let uv = in.uv;
    let tent = (src(uv, texel, -1.0, -1.0) + src(uv, texel, 1.0, -1.0) + src(uv, texel, -1.0, 1.0) + src(uv, texel, 1.0, 1.0)
        + (src(uv, texel, 0.0, -1.0) + src(uv, texel, -1.0, 0.0) + src(uv, texel, 1.0, 0.0) + src(uv, texel, 0.0, 1.0)) * 2.0
        + src(uv, texel, 0.0, 0.0) * 4.0) / 16.0;
    let base = clean(textureSampleLevel(t_base, s_lin, uv, 0.0).rgb);
    return vec4<f32>(mix(base, tent, 0.6), 1.0);
}

// --- automatic exposure: the mean log luminance of the smallest glow level, weighted to
// the middle of the picture, and its slow follower

@fragment
fn fs_meter(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(t_src));
    var sum = 0.0;
    var wsum = 0.0;
    for (var y = 0; y < dims.y; y = y + 1) {
        for (var x = 0; x < dims.x; x = x + 1) {
            let c = clean(textureLoad(t_src, vec2<i32>(x, y), 0).rgb);
            let q = (vec2<f32>(f32(x), f32(y)) + 0.5) / vec2<f32>(dims) * 2.0 - 1.0;
            // the middle and the lower half count most: the sky is not what one looks at
            let w = exp(-dot(q, q) * 1.5) * (0.6 + 0.4 * clamp(q.y + 0.5, 0.0, 1.0));
            sum = sum + log2(clamp(luma(c), 1e-4, 64.0)) * w;
            wsum = wsum + w;
        }
    }
    return vec4<f32>(sum / max(wsum, 1e-6), 0.0, 0.0, 1.0);
}

@fragment
fn fs_adapt(in: VsOut) -> @location(0) vec4<f32> {
    let now = textureLoad(t_src, vec2<i32>(0, 0), 0).r;
    let before_raw = textureLoad(t_base, vec2<i32>(0, 0), 0).r;
    // (a NaN once in the adaptation would stay there and turn the whole picture black)
    let before = select(now, before_raw, (bitcast<u32>(before_raw) & 0x7f800000u) != 0x7f800000u);
    // the eye takes to the light quickly and to the dark slowly
    var k = select(p.b.y, p.b.x, now < before);
    k = max(k, p.a.w);
    return vec4<f32>(mix(before, now, k), 0.0, 0.0, 1.0);
}

// --- the picture

fn pbr_neutral(color: vec3<f32>) -> vec3<f32> {
    let start = 0.8;
    let desat = 0.15;
    var c = color;
    let peak = max(c.r, max(c.g, c.b));
    if (peak < start) {
        return c;
    }
    let d = 1.0 - start;
    let new_peak = 1.0 - d * d / (peak + d - start);
    c = c * (new_peak / peak);
    let g = 1.0 - 1.0 / (desat * (peak - new_peak) + 1.0);
    return mix(c, vec3<f32>(new_peak), g);
}

// Night vision: where the scene is darker than a lit street (about a candela per square
// metre) the eye's rods take over from its cones: colours fade and what is left of them
// shifts towards blue-green (the Purkinje shift). A lamp's pool keeps its colour, the
// street beyond it turns to a bluish grey. `pre` is the pre-exposure, which gives the
// absolute luminance (1 = 10 000 cd/m²).
fn night_vision(c: vec3<f32>, strength: f32, pre: f32) -> vec3<f32> {
    let lum = luma(c);
    let cd = lum / max(pre, 1e-6) * 10000.0;
    // fully scotopic below 0.01 cd/m², photopic above 3
    let rods = 1.0 - smoothstep(-2.0, 0.5, log(max(cd, 1e-6)) / log(10.0));
    // the rods' sensitivity peaks at 507 nm: blue and green count, red hardly
    let scot = dot(c, vec3<f32>(0.05, 0.62, 0.45));
    let tint = vec3<f32>(0.86, 0.95, 1.12);
    let night = tint * scot * (lum / max(dot(tint * scot, vec3<f32>(0.2126, 0.7152, 0.0722)), 1e-8));
    return mix(c, night, rods * strength);
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn from_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow(max((c + 0.055) / 1.055, vec3<f32>(0.0)), vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn display_output(encoded: vec3<f32>) -> vec3<f32> {
    if (OUTPUT_SRGB) {
        return from_srgb(encoded);
    }
    return encoded;
}

fn graded(in: VsOut) -> vec3<f32> {
    let hdr = clean(textureSampleLevel(t_src, s_lin, in.uv, 0.0).rgb);
    let glow = clean(textureSampleLevel(t_base, s_lin, in.uv, 0.0).rgb);
    var c = mix(hdr, glow, p.a.x);
    let metered = textureLoad(t_adapt, vec2<i32>(0, 0), 0).r;
    let ev = clamp((p.b.z - metered) * p.c.x, -p.a.y, p.a.z) + p.b.w;
    c = max(c, vec3<f32>(0.0));
    if (p.c.y > 0.0) {
        c = night_vision(c, p.c.y, p.c.z);
    }
    c *= exp2(ev);
    // Open midtones uniformly across RGB; leave deep shadows and bright highlights alone
    let y = luma(c);
    let midtones = smoothstep(0.02, 0.12, y) * (1.0 - smoothstep(0.45, 1.0, y));
    c = pbr_neutral(c * exp2(0.18 * midtones));
    var e = to_srgb(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)));
    // triangular dither of one code value: no bands in the sky's gradient
    let px = in.clip.xy;
    let n1 = fract(52.9829189 * fract(dot(px, vec2<f32>(0.06711056, 0.00583715))));
    let n2 = fract(52.9829189 * fract(dot(px + vec2<f32>(17.0, 31.0), vec2<f32>(0.06711056, 0.00583715))));
    e = e + vec3<f32>((n1 + n2 - 1.0) / 255.0);
    return e;
}

// Straight to the display target, with exactly one sRGB encoding on store.
@fragment
fn fs_tonemap(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(display_output(clamp(graded(in), vec3<f32>(0.0), vec3<f32>(1.0))), 1.0);
}

// into the gamma-encoded target FXAA reads, with its luma in alpha
@fragment
fn fs_tonemap_encoded(in: VsOut) -> @location(0) vec4<f32> {
    let e = clamp(graded(in), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(e, dot(e, vec3<f32>(0.299, 0.587, 0.114)));
}

// --- FXAA 3.11 (quality, 12 search steps) over the tone-mapped picture

fn lum_at(uv: vec2<f32>) -> f32 {
    return textureSampleLevel(t_src, s_lin, uv, 0.0).a;
}

@fragment
fn fs_fxaa(in: VsOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(t_src));
    let uv = in.uv;
    let rgbm = textureSampleLevel(t_src, s_lin, uv, 0.0);
    // the bus's own screens as they are: FXAA took half the contrast out of their
    // letters (the screen mask is this pass's t_base)
    if (textureSampleLevel(t_base, s_lin, uv, 0.0).r > 0.5) {
        return vec4<f32>(display_output(rgbm.rgb), 1.0);
    }
    let m = rgbm.a;
    let n = lum_at(uv + vec2<f32>(0.0, -1.0) * texel);
    let s = lum_at(uv + vec2<f32>(0.0, 1.0) * texel);
    let e = lum_at(uv + vec2<f32>(1.0, 0.0) * texel);
    let w = lum_at(uv + vec2<f32>(-1.0, 0.0) * texel);
    let hi = max(m, max(max(n, s), max(e, w)));
    let lo = min(m, min(min(n, s), min(e, w)));
    let range = hi - lo;
    if (range < max(0.0312, hi * 0.125)) {
        return vec4<f32>(display_output(rgbm.rgb), 1.0);
    }
    let nw = lum_at(uv + vec2<f32>(-1.0, -1.0) * texel);
    let ne = lum_at(uv + vec2<f32>(1.0, -1.0) * texel);
    let sw = lum_at(uv + vec2<f32>(-1.0, 1.0) * texel);
    let se = lum_at(uv + vec2<f32>(1.0, 1.0) * texel);
    // sub-pixel aliasing
    let avg = (2.0 * (n + s + e + w) + nw + ne + sw + se) / 12.0;
    let sub = clamp(abs(avg - m) / range, 0.0, 1.0);
    let sub2 = smoothstep(0.0, 1.0, sub);
    let blend_sub = sub2 * sub2 * 0.75;
    // edge direction
    let horz = abs(nw + sw - 2.0 * w) + abs(n + s - 2.0 * m) * 2.0 + abs(ne + se - 2.0 * e);
    let vert = abs(nw + ne - 2.0 * n) + abs(w + e - 2.0 * m) * 2.0 + abs(sw + se - 2.0 * s);
    let is_h = horz >= vert;
    var stp = select(texel.x, texel.y, is_h);
    let l1 = select(w, n, is_h);
    let l2 = select(e, s, is_h);
    let g1 = abs(l1 - m);
    let g2 = abs(l2 - m);
    let neg = g1 >= g2;
    let grad = max(g1, g2) * 0.25;
    let edge_l = 0.5 * (m + select(l2, l1, neg));
    if (neg) {
        stp = -stp;
    }
    var cuv = uv;
    if (is_h) {
        cuv.y = cuv.y + stp * 0.5;
    } else {
        cuv.x = cuv.x + stp * 0.5;
    }
    let dir = select(vec2<f32>(0.0, texel.y), vec2<f32>(texel.x, 0.0), is_h);
    // walk along the edge both ways
    let steps = array<f32, 12>(1.0, 1.0, 1.0, 1.0, 1.0, 1.5, 2.0, 2.0, 2.0, 2.0, 4.0, 8.0);
    var p_uv = cuv + dir;
    var n_uv = cuv - dir;
    var p_end = lum_at(p_uv) - edge_l;
    var n_end = lum_at(n_uv) - edge_l;
    var p_done = abs(p_end) >= grad;
    var n_done = abs(n_end) >= grad;
    for (var i = 1; i < 12; i = i + 1) {
        if (p_done && n_done) {
            break;
        }
        if (!p_done) {
            p_uv = p_uv + dir * steps[i];
            p_end = lum_at(p_uv) - edge_l;
            p_done = abs(p_end) >= grad;
        }
        if (!n_done) {
            n_uv = n_uv - dir * steps[i];
            n_end = lum_at(n_uv) - edge_l;
            n_done = abs(n_end) >= grad;
        }
    }
    var dp: f32;
    var dn: f32;
    if (is_h) {
        dp = p_uv.x - uv.x;
        dn = uv.x - n_uv.x;
    } else {
        dp = p_uv.y - uv.y;
        dn = uv.y - n_uv.y;
    }
    let near_neg = dn <= dp;
    let end_l = select(p_end, n_end, near_neg);
    let span = dp + dn;
    var blend_edge = 0.0;
    // only when the end of the edge is on the other side of the middle
    if ((m - edge_l < 0.0) != (end_l < 0.0)) {
        blend_edge = 0.5 - min(dp, dn) / span;
    }
    let blend = max(blend_edge, blend_sub);
    var fuv = uv;
    if (is_h) {
        fuv.y = fuv.y + blend * stp;
    } else {
        fuv.x = fuv.x + blend * stp;
    }
    return vec4<f32>(display_output(textureSampleLevel(t_src, s_lin, fuv, 0.0).rgb), 1.0);
}
