// Enhanced graphics: the sky - the physical sky table, clouds lit by the sun and the sky,
// the sun's disc and the air - and the reflection probe drawn from it.

// Clouds: a ray-marched volume on the curved Earth, built the way Guerrilla's Horizon Zero
// Dawn clouds are (and bevy-volumetric-clouds and Frostbite after them):
// * the layer is a spherical shell 1400-2800 m over the ground of an Earth of 6371 km, so
//   the clouds sink to the horizon and thin out into the distance as real ones do;
// * the shape map (clouds.rs: Perlin cut by Worley cells, repeating every 13 km) says where
//   the heaps stand and how much cover each needs to appear; its blue channel and a height
//   gradient round their tops and flatten their bases; the weather's cover (Cumulus 1-3,
//   Overcast) and the weather's own cloud picture nudge how much of it appears;
// * the detail volume (3-D Worley billows, repeating every 420 m) eats the edges, most at
//   the thin rims and least in the dense cores;
// * light: the sun through the cloud towards it (six steps growing 1.7 times, 1.4 km in
//   all) with Hillaire's approximation of multiple scattering (three octaves of weaker
//   extinction and a flatter phase), a two-lobe Henyey-Greenstein phase (a silver lining
//   towards the sun, a little back-scatter away from it), the sky's light from above and
//   the ground's from below by height, integrated per step the energy-conserving
//   Frostbite way; far clouds take on the air's colour.
// The sky cube keeps the result in a fixed world frame (lib.rs `SKY_CUBE_SIZE`): each
// redraw starts the steps at another random point and is blended into what is there, so
// the grain of a few dozen steps averages out over the frames.
const CLOUD_BOTTOM: f32 = 1400.0;
const CLOUD_TOP: f32 = 4200.0;
const EARTH_R: f32 = 6371000.0;
const CLOUD_STEPS: i32 = 64;
// Extinction per metre of the densest cloud.
const CLOUD_SIGMA: f32 = 0.04;
const CLOUD_SHAPE_PERIOD: f32 = 13000.0;
const CLOUD_DETAIL_PERIOD: f32 = 420.0;
const CLOUD_DETAIL_STRENGTH: f32 = 0.3;
const CLOUD_EDGE_SOFTNESS: f32 = 0.16;
const CLOUD_BOTTOM_SOFTNESS: f32 = 0.2;
const CLOUD_MAX_DIST: f32 = 60000.0;
const CLOUD_MS_GAIN: f32 = 4.0;

@group(1) @binding(6) var t_cloud_shape: texture_2d<f32>;
@group(1) @binding(7) var t_cloud_detail: texture_3d<f32>;
@group(1) @binding(8) var s_cloud: sampler;

// Where in its first step this pixel's ray starts (0..1; 0.5 for the reflection probe).
var<private> cloud_jitter: f32 = 0.5;

fn linearstep(a: f32, b: f32, v: f32) -> f32 {
    return clamp((v - a) / (b - a), 0.0, 1.0);
}

// Height over the curved ground of the point t metres along d from the camera.
fn cloud_height(d: vec3<f32>, t: f32) -> f32 {
    return camera.cam_pos.z + eye_off.z + d.z * t + t * t * (1.0 - d.z * d.z) / (2.0 * EARTH_R);
}

// Distance along d at which the ray reaches height h over the curved ground (the far root
// when the camera is above h; -1 when it never does).
fn cloud_shell(d: vec3<f32>, h: f32) -> f32 {
    let a = (1.0 - d.z * d.z) / (2.0 * EARTH_R);
    let b = d.z;
    let c = camera.cam_pos.z + eye_off.z - h;
    if (a < 1e-12) {
        return select(-1.0, -c / b, abs(b) > 1e-6 && -c / b > 0.0);
    }
    let disc = b * b - 4.0 * a * c;
    if (disc < 0.0) {
        return -1.0;
    }
    let q = -0.5 * (b + select(-1.0, 1.0, b >= 0.0) * sqrt(disc));
    let r0 = q / a;
    let r1 = c / q;
    let lo = min(r0, r1);
    let hi = max(r0, r1);
    if (lo > 0.0) {
        return lo;
    }
    return select(-1.0, hi, hi > 0.0);
}

// How much of the sky the weather covers here: its type (camera.clouds.x) and, over tens
// of kilometres, its own cloud picture.
fn cloud_coverage(p: vec2<f32>) -> f32 {
    let cover = camera.clouds.x;
    let uv = p / CLOUD_FIELD_TILE + camera.clouds.yz * (2500.0 / CLOUD_FIELD_TILE);
    let field = textureSampleLevel(t_clouds, s_repeat, uv * 0.35, 5.0).g;
    return clamp(0.3 + cover * 0.55 + (field - 0.5) * 0.25, 0.0, 1.0);
}

// The heap before its billows (h: 0 at the base, 1 at the top of the layer).
fn cloud_base_shape(p: vec3<f32>, h: f32, lod: f32) -> f32 {
    let drift = camera.clouds.yz * 2500.0;
    let s = textureSampleLevel(t_cloud_shape, s_cloud, (p.xy + drift) / CLOUD_SHAPE_PERIOD, lod);
    let lo = s.g - 1.0;
    // the heap narrows upwards (every heap: with the map's rounding alone, a heap where it
    // was small rose as a column with a flat lid against the top of the layer)
    let n = h * h * (0.7 + s.b) + pow(1.0 - h, 16.0);
    let m = (s.r - n - lo) / (1.0 - lo);
    return m * (linearstep(0.0, 0.1, h) - linearstep(0.6, 1.0, h));
}

// Extinction (1/m) at p; `detail` false for the light towards the sun far from the point.
fn cloud_sigma(p: vec3<f32>, h: f32, coverage: f32, lod: f32, detail: bool) -> f32 {
    if (h <= 0.0 || h >= 1.0) {
        return 0.0;
    }
    var m = cloud_base_shape(p, h, lod);
    // (the billows only take away)
    if (m + coverage - 1.0 <= 0.0) {
        return 0.0;
    }
    if (detail) {
        let drift = camera.clouds.yz * 2500.0 * 1.3;
        let q = vec3<f32>(p.xy + drift, p.z) / CLOUD_DETAIL_PERIOD;
        let dl = textureSampleLevel(t_cloud_detail, s_cloud, q, max(lod - 2.0, 0.0)).r;
        m = m - dl * smoothstep(1.0, 0.5, m) * CLOUD_DETAIL_STRENGTH;
    }
    m = smoothstep(0.0, CLOUD_EDGE_SOFTNESS, m + coverage - 1.0);
    m = m * min(h / CLOUD_BOTTOM_SOFTNESS, 1.0);
    return m * CLOUD_SIGMA;
}

// The clouds towards d in front of `below` (the sky behind them): rgb the picture, a how
// much the clouds cover.
fn cloud_layer(d: vec3<f32>, below: vec3<f32>, pix: f32) -> vec4<f32> {
    if (camera.clouds.x <= 0.001 || d.z <= -0.01 || camera.cam_pos.z + eye_off.z > CLOUD_BOTTOM) {
        return vec4<f32>(below, 0.0);
    }
    let sd = normalize(camera.sun_dir.xyz);
    // a closed cover (or a sky that rain or snow falls from) is the grey deck of the table
    let closed = max(smoothstep(0.85, 1.0, camera.clouds.x), enh.weather.w);
    let t0 = cloud_shell(d, CLOUD_BOTTOM);
    if (t0 < 0.0 || t0 > CLOUD_MAX_DIST) {
        return vec4<f32>(below, 0.0);
    }
    let t1 = min(cloud_shell(d, CLOUD_TOP), min(t0 + 12000.0, CLOUD_MAX_DIST + 6000.0));
    let ds = (t1 - t0) / f32(CLOUD_STEPS);
    // how many texels of the shape map a pixel spans where the ray meets the clouds
    let lod = log2(max(t0 * pix * f32(textureDimensions(t_cloud_shape).x) / CLOUD_SHAPE_PERIOD, 1.0));
    // (the sun before the clouds: how much of it the cover lets through, lights.w, is what
    // this march works out itself)
    let sun = enh.sun_disc.rgb * smoothstep(-0.08, 0.02, sd.z);
    let sky_top = sh_irradiance(vec3<f32>(0.0, 0.0, 1.0)) / PI;
    let ground = sh_irradiance(vec3<f32>(0.0, 0.0, -1.0)) / PI;
    let cos_sun = dot(d, sd);
    let coverage = mix(cloud_coverage(cloud_ground(d, t0)), 1.0, closed);
    var trans = 1.0;
    var acc = vec3<f32>(0.0);
    var hit = 0.0;
    var hit_w = 0.0;
    var t = t0 + ds * cloud_jitter;
    for (var i = 0; i < CLOUD_STEPS; i = i + 1) {
        let p = vec3<f32>(cloud_ground(d, t), cloud_height(d, t));
        let h = (p.z - CLOUD_BOTTOM) / (CLOUD_TOP - CLOUD_BOTTOM);
        let sigma = cloud_sigma(p, h, coverage, lod, true);
        if (sigma > 1e-6) {
            // the sunlight reaching p through the cloud towards the sun
            var od = 0.0;
            var ls = 40.0;
            var lt = ls * 0.5;
            for (var k = 0; k < 6; k = k + 1) {
                let q = p + sd * lt;
                let hq = (q.z - CLOUD_BOTTOM) / (CLOUD_TOP - CLOUD_BOTTOM);
                if (hq >= 1.0) {
                    break;
                }
                od = od + cloud_sigma(q, hq, coverage, lod + 1.0, k < 2) * ls;
                ls = ls * 1.7;
                lt = lt + ls;
            }
            // multiple scattering (Hillaire 2016): octaves of weaker extinction, weaker
            // light and a flatter phase
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
            // (single scattering and three octaves hold only part of the light a cloud
            // scatters on inside it; a cumulus's sunlit side is about as bright as white
            // paper in the sun, E/π, which this factor brings it to)
            let amb = mix(ground * 0.45 + sky_top * 0.55, sky_top * 1.1, clamp(h * 1.4, 0.0, 1.0));
            let powder = mix(1.0, 1.0 - exp(-sigma * 600.0), 0.5);
            let light = (direct * powder * CLOUD_MS_GAIN + amb * 1.6 * mix(0.55, 1.0, smoothstep(0.0, 0.55, h))) * (1.0 - 0.3 * closed);
            // Frostbite: the light scattered over the step, dimmed by the cloud before it
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
    // far clouds take on the colour of the air in front of them, and beyond 40 km fade out
    let dist = select(t0, hit / max(hit_w, 1e-4), hit_w > 1e-4);
    let aerial = 1.0 - exp(-dist / 22000.0);
    let fade = 1.0 - smoothstep(40000.0, CLOUD_MAX_DIST, dist);
    let horizon_fade = smoothstep(-0.01, 0.02, d.z);
    let a = (1.0 - trans) * fade * horizon_fade;
    acc = mix(acc, below * (1.0 - trans), aerial) * fade * horizon_fade;
    var col = acc + (1.0 - a) * below;
    return vec4<f32>(col, a);
}

// Sky radiance towards d without the sun's disc: the table, the clouds, the air.
fn sky_radiance(d: vec3<f32>, pix: f32) -> vec3<f32> {
    var col = sky_table(d);
    let c = cloud_layer(d, col, pix);
    col = c.rgb;
    // the weather's fog along the whole way to the horizon (the clear air is in the table
    // already: taken again, it whitened the whole sky)
    let h0 = camera.cam_pos.z - enh.fog.z;
    let dist = 30000.0;
    let a = air_of(d, dist, h0, h0 + dist * max(d.z, 0.0), 0.0);
    return col * a.a + a.rgb;
}

// The sky as the sky cube holds it (drawn a face a frame by `fs_sky_cube`, divided by the
// table scale): rgb, and how much cloud covers the direction.
@group(0) @binding(17) var t_sky_cube: texture_cube<f32>;

@fragment
fn fs_enhanced(in: VsOut) -> @location(0) vec4<f32> {
    let d = normalize(in.dir);
    let star_pix = length(fwidth(d));
    let pre = enh.exposure.x;
    // the cube is drawn from its own eye (lib.rs Probe::cube_eye): look the clouds' base up
    // from there, so the sky does not slide with a camera that moved since
    var ld = d;
    let tb = cloud_shell(d, CLOUD_BOTTOM);
    if (tb > 0.0 && camera.cam_pos.z < CLOUD_BOTTOM) {
        ld = normalize(d * min(tb, CLOUD_MAX_DIST) - enh.eye.xyz);
    }
    let cube = textureSampleLevel(t_sky_cube, s_lin, vec3<f32>(ld.x, ld.z, ld.y), 0.0);
    var col = cube.rgb * enh.ground.w;
    // the sun: a limb-darkened disc as bright as its irradiance spread over its size,
    // behind whatever cloud there is
    let sd = normalize(camera.sun_dir.xyz);
    let r = enh.sun.w;
    let cosang = dot(d, sd);
    if (cosang > cos(r * 1.5) && d.z > -0.02) {
        let x = clamp(acos(clamp(cosang, -1.0, 1.0)) / r, 0.0, 1.5);
        let disc = 1.0 - smoothstep(0.85, 1.1, x);
        let limb = 1.0 - 0.6 * (1.0 - sqrt(max(1.0 - min(x, 1.0) * min(x, 1.0), 0.0)));
        let cover = cube.a;
        let h0 = camera.cam_pos.z - enh.fog.z;
        let t = air_of(d, 30000.0, h0, h0 + 30000.0 * max(d.z, 0.0), 0.0).a;
        let l = enh.sun_disc.rgb / (PI * r * r) * disc * limb * (1.0 - cover) * enh.lights.w * t;
        col = col + l;
    }
    // the dome is drawn pre-exposed; the disc is kept within what the target and the glow
    // filter handle
    // (with the night at brightness 0 the table scale is ~0 and the cube holds huge values:
    // a non-finite pixel must not take the stars with it)
    let col_bits = bitcast<vec3<u32>>(col) & vec3<u32>(0x7f800000u);
    if (!all(col_bits != vec3<u32>(0x7f800000u))) {
        col = vec3<f32>(0.0);
    }
    var out_col = min(col * pre, vec3<f32>(4000.0));
    // stars: added in picture units, so neither the night's brightness setting nor the
    // exposure dims them; only at night, above the horizon, behind clouds and fog
    {
        let night_f = 1.0 - smoothstep(-0.25, -0.05, sd.z);
        if (night_f > 0.001 && d.z > 0.0) {
            let h0s = camera.cam_pos.z - enh.fog.z;
            let ts = air_of(d, 30000.0, h0s, h0s + 30000.0 * max(d.z, 0.0), 0.0).a;
            let vis = night_f * smoothstep(0.0, 0.08, d.z) * (1.0 - cube.a) * ts;
            out_col = out_col + star_field(d, star_pix, camera.post.y, camera.sky.x) * vis * 0.6;
        }
    }
    return vec4<f32>(out_col, 1.0);
}

// --- the reflection probe: a cube map of the sky seen from the camera, drawn now and then
// and blurred into its mip levels for rough surfaces (divided by the table scale, which
// the main pass multiplies back in).
struct ProbeParams {
    // x first face of this pass (0 or 3), y roughness, z face size of level 0, w level
    p: vec4<f32>,
};
@group(0) @binding(15) var<uniform> probe: ProbeParams;
@group(0) @binding(16) var t_probe_src: texture_cube<f32>;

// Direction of a texel of cube face f (u, v in -1..1, v down), in cube coordinates.
fn face_dir(f: i32, u: f32, v: f32) -> vec3<f32> {
    var c = vec3<f32>(-u, -v, -1.0);
    switch f {
        case 0: { c = vec3<f32>(1.0, -v, -u); }
        case 1: { c = vec3<f32>(-1.0, -v, u); }
        case 2: { c = vec3<f32>(u, 1.0, v); }
        case 3: { c = vec3<f32>(u, -1.0, -v); }
        case 4: { c = vec3<f32>(u, -v, 1.0); }
        default: {}
    }
    return normalize(c);
}

fn cube_to_world(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(c.x, c.z, c.y);
}

struct ProbeOut {
    @location(0) a: vec4<f32>,
    @location(1) b: vec4<f32>,
    @location(2) c: vec4<f32>,
};

struct FsIn {
    @builtin(position) clip: vec4<f32>,
};

@vertex
fn vs_probe(@builtin(vertex_index) i: u32) -> FsIn {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return FsIn(vec4<f32>(x, y, 0.0, 1.0));
}

fn probe_sky(f: i32, uv: vec2<f32>) -> vec4<f32> {
    let c = face_dir(f, uv.x, uv.y);
    let d = cube_to_world(c);
    var col: vec3<f32>;
    if (d.z >= 0.0) {
        // the sky cube (lib.rs binds it as level 0's source), already divided by the table
        // scale: marching the clouds again for the probe took 3.4 ms every time it was drawn
        return vec4<f32>(textureSampleLevel(t_probe_src, s_lin, c, 0.0).rgb, 1.0);
    } else {
        // below the horizon: the table's distant ground, with the houses and trees of a
        // town darkening the band just under the horizon
        col = sky_table(d);
        let band = smoothstep(0.02, 0.25, -d.z);
        col = mix(col, enh.ground.rgb * 0.8, 1.0 - band * 0.6);
    }
    return vec4<f32>(col / max(enh.ground.w, 1e-8), 1.0);
}

@fragment
fn fs_probe_sky(in: FsIn) -> ProbeOut {
    let size = probe.p.z;
    let uv = in.clip.xy / size * 2.0 - vec2<f32>(1.0);
    let f0 = i32(probe.p.x);
    return ProbeOut(probe_sky(f0, uv), probe_sky(f0 + 1, uv), probe_sky(f0 + 2, uv));
}

fn radical_inverse(i: u32) -> f32 {
    var b = i;
    b = (b << 16u) | (b >> 16u);
    b = ((b & 0x55555555u) << 1u) | ((b & 0xAAAAAAAAu) >> 1u);
    b = ((b & 0x33333333u) << 2u) | ((b & 0xCCCCCCCCu) >> 2u);
    b = ((b & 0x0F0F0F0Fu) << 4u) | ((b & 0xF0F0F0F0u) >> 4u);
    b = ((b & 0x00FF00FFu) << 8u) | ((b & 0xFF00FF00u) >> 8u);
    return f32(b) * 2.3283064365386963e-10;
}

// GGX-filtered radiance around n (the split-sum assumption: view = normal), sampling the
// sharper levels at a lod that matches each sample's footprint.
fn prefilter(n: vec3<f32>) -> vec3<f32> {
    let rough = probe.p.y;
    let a = rough * rough;
    let up = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), abs(n.z) < 0.999);
    let tx = normalize(cross(up, n));
    let ty = cross(n, tx);
    let count = 32u;
    let texel_sa = 4.0 * PI / (6.0 * probe.p.z * probe.p.z);
    var sum = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var i = 0u; i < count; i = i + 1u) {
        let xi = vec2<f32>(f32(i) / f32(count), radical_inverse(i));
        let phi = 2.0 * PI * xi.x;
        let cos_t = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
        let sin_t = sqrt(1.0 - cos_t * cos_t);
        let h = tx * (cos(phi) * sin_t) + ty * (sin(phi) * sin_t) + n * cos_t;
        let l = 2.0 * dot(n, h) * h - n;
        let nl = dot(n, l);
        if (nl <= 0.0) {
            continue;
        }
        let nh = max(dot(n, h), 0.0);
        let d2 = nh * nh * (a * a - 1.0) + 1.0;
        let dd = a * a / (PI * d2 * d2);
        let pdf = dd * 0.25;
        let sample_sa = 1.0 / (f32(count) * pdf + 1e-4);
        let lod = clamp(0.5 * log2(sample_sa / texel_sa) + 1.0, 0.0, probe.p.w - 1.0);
        sum = sum + textureSampleLevel(t_probe_src, s_lin, l, lod).rgb * nl;
        wsum = wsum + nl;
    }
    return sum / max(wsum, 1e-4);
}

@fragment
fn fs_probe_filter(in: FsIn) -> ProbeOut {
    let size = probe.p.z / pow(2.0, probe.p.w);
    let uv = in.clip.xy / size * 2.0 - vec2<f32>(1.0);
    let f0 = i32(probe.p.x);
    return ProbeOut(
        vec4<f32>(prefilter(face_dir(f0, uv.x, uv.y)), 1.0),
        vec4<f32>(prefilter(face_dir(f0 + 1, uv.x, uv.y)), 1.0),
        vec4<f32>(prefilter(face_dir(f0 + 2, uv.x, uv.y)), 1.0),
    );
}

// One face of the sky cube: the sky radiance (the table, the clouds, the air) divided by the
// table scale, and the cloud cover in alpha (the sun's disc is hidden by it).
@fragment
fn fs_sky_cube(in: FsIn) -> @location(0) vec4<f32> {
    let size = probe.p.z;
    let uv = in.clip.xy / size * 2.0 - vec2<f32>(1.0);
    let d = cube_to_world(face_dir(i32(probe.p.x), uv.x, uv.y));
    let pix = 1.4 / size;
    // a new start point for the steps each redraw (p.y counts the redraws): interleaved
    // gradient noise over the face, shifted by the golden ratio
    let ign = fract(52.9829189 * fract(dot(in.clip.xy, vec2<f32>(0.06711056, 0.00583715))));
    cloud_jitter = fract(ign + probe.p.y * 0.61803399);
    eye_off = enh.eye.xyz;
    var col = sky_table(d);
    let c = cloud_layer(d, col, pix);
    col = c.rgb;
    let h0 = camera.cam_pos.z + eye_off.z - enh.fog.z;
    let dist = 30000.0;
    let a = air_of(d, dist, h0, h0 + dist * max(d.z, 0.0), 0.0);
    col = col * a.a + a.rgb;
    return vec4<f32>(col / max(enh.ground.w, 1e-8), c.a);
}
