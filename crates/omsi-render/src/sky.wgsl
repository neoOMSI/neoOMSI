// Sky dome: the envir.cfg gradient textures (day / twilight / night), u = azimuth relative
// to the sun, v = elevation (0 zenith … 1 horizon), blended by the sun altitude.
struct Camera {
    view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    // Kept in lockstep with `CameraUniform` in lib.rs.  The sky does not use the
    // floating-origin reconstruction itself, but omitting this member shifts every
    // following field one vec4 earlier: vanilla then reads the light-grid as `sky`
    // weights (usually all zero) and clears/draws a black sky.
    world_origin: vec4<f32>,
    sun_dir: vec4<f32>,
    ambient: vec4<f32>,
    fog: vec4<f32>,
    sun_color: vec4<f32>,
    sky_color: vec4<f32>,
    light_grid: vec4<f32>,
    sky: vec4<f32>,          // x sun azimuth (rad), y day weight, z twilight weight, w night weight
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    clouds: vec4<f32>,       // x density 0..1, yz texture offset (wind drift)
    light_view_proj: mat4x4<f32>,
    light_view_proj_far: mat4x4<f32>,
    shadow: vec4<f32>,
    post: vec4<f32>,         // x enhanced, y time
    inside_a: vec4<f32>,
    inside_b: vec4<f32>,
    inside_c: vec4<f32>,
    flags: vec4<f32>,
    light_view_proj_close: mat4x4<f32>,
    wind: vec4<f32>,
};

fn hash2(q: vec2<f32>) -> f32 {
    return fract(sin(dot(q, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2<f32>(1.0, 0.0)), u.x), mix(hash2(i + vec2<f32>(0.0, 1.0)), hash2(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

// Cloud density field: the map's cloud texture as the large shape, fractal noise as the
// small one, two layers at different heights drifting at different speeds.
fn cloud_fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var q = p;
    for (var i = 0; i < 4; i = i + 1) {
        v = v + a * vnoise(q);
        q = q * 2.1 + vec2<f32>(17.0, 9.0);
        a = a * 0.5;
    }
    return v;
}
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var t_day: texture_2d<f32>;
@group(1) @binding(1) var t_twilight: texture_2d<f32>;
@group(1) @binding(2) var t_night: texture_2d<f32>;
@group(1) @binding(3) var s_sky: sampler;
@group(1) @binding(4) var t_clouds: texture_2d<f32>;
@group(1) @binding(5) var s_repeat: sampler;

/// How far the cloud field reaches before it repeats (m): a cumulus is then half a
/// kilometre to two across.
const CLOUD_FIELD_TILE: f32 = 14000.0;

// The ground point under the sky at distance `t` along the view ray `d`, in the clouds' own
// frame (world coordinates modulo 70 km, lib.rs `CLOUD_ORIGIN_PERIOD`), so that the cloud
// field stays where it is when the floating render origin moves on.
// Where the clouds are seen from, relative to the camera (the enhanced sky cube's own eye
// while it is drawn; 0 everywhere else).
var<private> eye_off: vec3<f32> = vec3<f32>(0.0);

fn cloud_ground(d: vec3<f32>, t: f32) -> vec2<f32> {
    return camera.cam_pos.xy + eye_off.xy + camera.world_origin.zw + d.xy * t;
}

// The cloud cover over ground point p (x, 0..1, its edges frayed by the billows), how tall
// the cloud there grows (y, 0..1) and the cover without the billows (z: the enhanced sky
// raises its rounded tops on that, the billows would stand on them as spikes). From the
// cloud field: its equalised shape (G) cut at 1 - the cover, the weather's own picture (R)
// nudging where the clouds gather, billows (B) fraying the edges near by.
fn cloud_cover_at(p: vec2<f32>, lod: f32) -> vec3<f32> {
    let uv = p / CLOUD_FIELD_TILE + camera.clouds.yz * (2500.0 / CLOUD_FIELD_TILE);
    let t = textureSampleLevel(t_clouds, s_repeat, uv, lod);
    let thr = 1.0 - clamp(camera.clouds.x, 0.0, 1.0);
    let smooth_shape = t.g + (t.r - 0.5) * 0.15;
    var shape = smooth_shape;
    // the billows only where they are big enough to see
    let fray = 1.0 - smoothstep(2.0, 5.0, lod);
    if (fray > 0.0) {
        let detail = textureSampleLevel(t_clouds, s_repeat, uv * 3.7 + vec2<f32>(0.31, 0.73), lod + 1.9).b;
        shape = shape + (detail - 0.5) * 0.2 * fray;
    }
    return vec3<f32>(clamp((shape - thr) / 0.14, 0.0, 1.0), t.a, clamp((smooth_shape - thr) / 0.14, 0.0, 1.0));
}

fn cloud_layer_basic(d: vec3<f32>, below: vec3<f32>, pix: f32, jitter: f32) -> vec4<f32> {
    if (camera.clouds.x <= 0.001 || d.z <= -0.01 || camera.cam_pos.z + eye_off.z > CLOUD_BOTTOM) {
        return vec4<f32>(below, 0.0);
    }
    let t0 = cloud_shell(d, CLOUD_BOTTOM);
    if (t0 < 0.0 || t0 > CLOUD_MAX_DIST) {
        return vec4<f32>(below, 0.0);
    }
    let sd = normalize(camera.sun_dir.xyz);
    let closed = smoothstep(0.8, 1.0, camera.clouds.x);
    let t1 = min(cloud_shell(d, CLOUD_TOP), min(t0 + 12000.0, CLOUD_MAX_DIST + 6000.0));
    let steps = 56;
    let ds = (t1 - t0) / f32(steps);
    let lod = log2(max(t0 * pix * f32(textureDimensions(t_cloud_shape).x) / CLOUD_SHAPE_PERIOD, 1.0));
    let sun_up = clamp(sd.z * 4.0 + 0.3, 0.0, 1.0);
    let sun = camera.sun_color.rgb * 1.3 * sun_up;
    let sky_top = camera.ambient.rgb * mix(0.5, 1.5, sun_up) + camera.sky_color.rgb * 0.3;
    let ground = camera.ambient.rgb * 0.5 * mix(0.4, 1.0, sun_up);
    let cos_sun = dot(d, sd);
    var coverage = cloud_coverage(cloud_ground(d, t0));
    coverage = mix(coverage, 1.0, closed);
    var trans = 1.0;
    var acc = vec3<f32>(0.0);
    var hit = 0.0;
    var hit_w = 0.0;
    var t = t0 + ds * jitter;
    for (var i = 0; i < steps; i = i + 1) {
        let p = vec3<f32>(cloud_ground(d, t), cloud_height(d, t));
        let h = (p.z - CLOUD_BOTTOM) / (CLOUD_TOP - CLOUD_BOTTOM);
        let sigma = cloud_sigma(p, h, coverage, lod, true);
        if (sigma > 1e-6) {
            var od = 0.0;
            var ls = 40.0;
            var lt = ls * 0.5;
            for (var k = 0; k < 5; k = k + 1) {
                let q = p + sd * lt;
                let hq = (q.z - CLOUD_BOTTOM) / (CLOUD_TOP - CLOUD_BOTTOM);
                if (hq >= 1.0) {
                    break;
                }
                od = od + cloud_sigma(q, hq, coverage, lod + 1.0, k < 2) * ls;
                ls = ls * 1.8;
                lt = lt + ls;
            }
            var direct = vec3<f32>(0.0);
            var a = 1.0;
            var b = 1.0;
            var c = 1.0;
            for (var o = 0; o < 3; o = o + 1) {
                let phase = mix(hg_phase(cos_sun, 0.8 * c), hg_phase(cos_sun, -0.2 * c), 0.5);
                direct = direct + sun * a * phase * exp(-od * b);
                a = a * 0.6;
                b = b * 0.3;
                c = c * 0.5;
            }
            let amb = mix(ground * 0.6 + sky_top * 0.5, sky_top * 1.1, clamp(h * 1.4, 0.0, 1.0));
            let powder = mix(1.0, 1.0 - exp(-sigma * 600.0), 0.5);
            let light = (direct * powder * CLOUD_MS_GAIN * PI + amb * 1.6 * mix(0.55, 1.0, smoothstep(0.0, 0.55, h))) * (1.0 - 0.3 * closed);
            let dt = exp(-sigma * ds);
            acc = acc + trans * light * (1.0 - dt);
            hit = hit + t * trans * (1.0 - dt);
            hit_w = hit_w + trans * (1.0 - dt);
            trans = trans * dt;
            if (trans < 0.01) {
                break;
            }
        }
        t = t + ds;
    }
    let dist = select(t0, hit / max(hit_w, 1e-4), hit_w > 1e-4);
    let aerial = 1.0 - exp(-dist / 22000.0);
    let fade = 1.0 - smoothstep(40000.0, CLOUD_MAX_DIST, dist);
    let horizon_fade = smoothstep(-0.01, 0.02, d.z);
    let alpha = (1.0 - trans) * fade * horizon_fade;
    let lit = mix(min(acc, vec3<f32>(1.6)), below * (1.0 - trans), aerial) * fade * horizon_fade;
    return vec4<f32>(lit + (1.0 - alpha) * below, alpha);
}

fn star_hash3(p: vec3<f32>) -> vec3<f32> {
    let c = bitcast<vec3<u32>>(vec3<i32>(floor(p)));
    var v = c * 1664525u + vec3<u32>(1013904223u);
    v.x = v.x + v.y * v.z;
    v.y = v.y + v.z * v.x;
    v.z = v.z + v.x * v.y;
    v = v ^ (v >> vec3<u32>(16u));
    v.x = v.x + v.y * v.z;
    v.y = v.y + v.z * v.x;
    v.z = v.z + v.x * v.y;
    return vec3<f32>(v >> vec3<u32>(8u)) * (1.0 / 16777216.0);
}

fn star_field(d: vec3<f32>, pix: f32, time: f32, sun_az: f32) -> vec3<f32> {
    let ca = cos(sun_az);
    let sa = sin(sun_az);
    let q = vec3<f32>(d.x * ca - d.y * sa, d.x * sa + d.y * ca, d.z);
    let S = 110.0;
    let p = q * S;
    let cell = floor(p);
    let h = star_hash3(cell);
    if (h.x > 0.16) {
        return vec3<f32>(0.0);
    }
    let r = star_hash3(cell + vec3<f32>(7.0, 3.0, 5.0));
    let pos = cell + vec3<f32>(0.3) + r * 0.4;
    let dist = length(p - pos);
    let rad = clamp(pix * S, 0.045, 0.1);
    let bright = 0.25 + 1.6 * h.y * h.y * h.y;
    let tw = 0.85 + 0.15 * sin(time * (2.0 + 4.0 * h.z) + h.y * 40.0);
    let peak = bright * tw * min(1.0, (0.06 / rad) + 0.35);
    var k = exp(-(dist * dist) / (2.0 * rad * rad * 0.25));
    let f = fract(p);
    let e = min(min(f.x, 1.0 - f.x), min(min(f.y, 1.0 - f.y), min(f.z, 1.0 - f.z)));
    k = k * smoothstep(0.0, 0.2, e);
    let tint = mix(vec3<f32>(0.75, 0.85, 1.0), vec3<f32>(1.0, 0.85, 0.65), r.z);
    return tint * peak * k;
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) dir: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>) -> VsOut {
    var out: VsOut;
    // the dome rides with the camera; pushed to the far plane (0 with reversed Z)
    let wp = camera.cam_pos.xyz + pos * 4000.0;
    var clip = camera.view_proj * vec4<f32>(wp, 1.0);
    clip.z = clip.w * 0.000001;
    out.clip = clip;
    out.dir = pos;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let d = normalize(in.dir);
    let star_pix = length(fwidth(d));
    let az = atan2(d.x, d.y);
    let u = fract((az - camera.sky.x) / 6.2831853 + 0.5);
    let elev = asin(clamp(d.z, -1.0, 1.0));
    // horizon row at the bottom; below the horizon keep the horizon colour
    let v = clamp(1.0 - elev / 1.5707963, 0.0, 0.995);
    let uv = vec2<f32>(u, v);
    let c = textureSample(t_day, s_sky, uv).rgb * camera.sky.y + textureSample(t_twilight, s_sky, uv).rgb * camera.sky.z * 0.8 + textureSample(t_night, s_sky, uv).rgb * camera.sky.w * 0.6;
    var col = c;
    // stars at night, fading out with the night weight, horizon and cloud cover
    let star_vis = (1.0 - smoothstep(-0.25, -0.05, camera.sun_dir.z)) * smoothstep(0.0, 0.08, d.z) * (1.0 - 0.9 * clamp(camera.clouds.x, 0.0, 1.0));
    var cloud_a = 0.0;
    if (camera.clouds.x > 0.001 && d.z > -0.01) {
        let ign = fract(52.9829189 * fract(dot(in.clip.xy, vec2<f32>(0.06711056, 0.00583715))));
        let cl = cloud_layer_basic(d, col, length(fwidth(d)), ign);
        col = cl.rgb;
        cloud_a = cl.a;
    }
    
    if (star_vis > 0.001) {
        col = col + star_field(d, star_pix, camera.post.y, camera.sky.x) * star_vis * (1.0 - cloud_a);
    }
    // fog swallows the horizon, and a thick fog (a few hundred metres of sight) the whole
    // sky: the blue does not show through ground fog
    let fw = select(0.0, camera.fog.w, camera.fog.w > 5e-4);
    let horizon = clamp(1.0 - elev / 0.12, 0.0, 1.0) * clamp(fw * 1500.0, 0.0, 1.0);
    let whole = clamp(fw * 150.0 - 0.15, 0.0, 1.0) * clamp(1.0 - elev / 1.2, 0.35, 1.0);
    let f = max(horizon, whole);
    return vec4<f32>(mix(col, camera.fog.xyz, f), 1.0);
}
