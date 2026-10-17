// Enhanced graphics: what the enhanced main pass, the sky and the coronas share - the
// light of the physically based sky (see atmosphere.rs), in its units (1 = 10 000 lux),
// and the air between the camera and what it sees. The scene is drawn pre-exposed:
// radiance times `enh.exposure.x`, so that the half-float target keeps its precision by
// night as well as by day.
struct Enhanced {
    // x pre-exposure, y the level unlit things are drawn at, z a lit window's radiance
    // (pre-exposed), w how bright a light's corona is drawn
    exposure: vec4<f32>,
    // rgb irradiance of the sun after the clouds, w its angular radius (rad)
    sun: vec4<f32>,
    // irradiance from the sky and the ground: order-2 spherical harmonics, rgb
    sh: array<vec4<f32>, 9>,
    // rgb radiance of the ground, w the sky table's scale
    ground: vec4<f32>,
    // x weather fog extinction (1/m) at its base, y its height falloff (1/m), z the base
    // height (render-origin relative), w clear-air extinction (1/m) at the ground
    fog: vec4<f32>,
    // rgb fog radiance from the light around it (the sun's part is added by angle),
    // w the table scale the reflection probe was drawn with
    fog_color: vec4<f32>,
    // x wet roads, y snow cover, z rain, w cloud cover in front of the sun
    weather: vec4<f32>,
    // x mip levels of the probe, y illuminance of a light's core (maplight colour 1),
    // z cabin light illuminance, w how much of the sun the clouds let through
    lights: vec4<f32>,
    // rgb the sun disc's irradiance before the clouds, w frame time (s)
    sun_disc: vec4<f32>,
    // x which term the main pass shows alone (OMSI_DEBUG_ENHANCED, 0 = the picture)
    debug: vec4<f32>,
    // xyz where the sky cube is drawn from, relative to the camera
    eye: vec4<f32>,
    // x how bright an LED panel's dots burn (0 = off), y whether the LED panels' `\S:n`
    // masks keep their mip chain (0: at full resolution, the dots stay visible when small)
    led: vec4<f32>,
    // up to three cloud layers, low to high: x base, y top (m), z cover 0..1, w shape
    // (0 flat stratus .. 1 piled cumulus)
    layers: array<vec4<f32>, 3>,
    rain: array<vec4<f32>, 64>,
    wet_info: vec4<f32>,
    wet: array<vec4<f32>, 64>,
};
@group(0) @binding(11) var<uniform> enh: Enhanced;
@group(0) @binding(13) var s_lin: sampler;
@group(0) @binding(14) var t_sky_lut: texture_2d<f32>;

const PI: f32 = 3.14159265;

fn sh_irradiance(n: vec3<f32>) -> vec3<f32> {
    let c = enh.sh;
    let e = c[0].rgb * 0.282095
        + c[1].rgb * (0.488603 * n.y) + c[2].rgb * (0.488603 * n.z) + c[3].rgb * (0.488603 * n.x)
        + c[4].rgb * (1.092548 * n.x * n.y) + c[5].rgb * (1.092548 * n.y * n.z)
        + c[6].rgb * (0.315392 * (3.0 * n.z * n.z - 1.0)) + c[7].rgb * (1.092548 * n.x * n.z)
        + c[8].rgb * (0.546274 * (n.x * n.x - n.y * n.y));
    return max(e, vec3<f32>(0.0));
}

// Sky radiance towards d (unscaled: times enh.ground.w is the radiance). The table runs
// over the azimuth from the sun (0..π) and the elevation (denser at the horizon).
fn sky_table_raw(d: vec3<f32>) -> vec3<f32> {
    let s = camera.sun_dir.xyz;
    let ls = length(s.xy);
    let ld = length(d.xy);
    var az = 0.0;
    if (ls > 1e-4 && ld > 1e-4) {
        az = acos(clamp(dot(s.xy / ls, d.xy / ld), -1.0, 1.0));
    }
    let el = asin(clamp(d.z, -1.0, 1.0));
    let v = 0.5 + 0.5 * sign(el) * sqrt(min(abs(el) / 1.5707963, 1.0));
    return textureSampleLevel(t_sky_lut, s_lin, vec2<f32>(az / PI, v), 0.0).rgb;
}

fn sky_table(d: vec3<f32>) -> vec3<f32> {
    return sky_table_raw(d) * enh.ground.w;
}

fn hg_phase(c: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI * pow(max(1.0 + g2 - 2.0 * g * c, 1e-4), 1.5));
}

// Optical depth of an exponentially thinning layer (extinction sigma at height 0, falling
// off with k per metre) along a straight path of length dist from height h0 to h1.
fn layer_depth(sigma: f32, k: f32, h0: f32, h1: f32, dist: f32) -> f32 {
    if (sigma <= 0.0) {
        return 0.0;
    }
    let a = max(h0, 0.0);
    let b = max(h1, 0.0);
    let dh = b - a;
    if (abs(k * dh) < 1e-3) {
        return sigma * dist * exp(-k * 0.5 * (a + b));
    }
    return sigma * dist * (exp(-k * a) - exp(-k * b)) / (k * dh);
}

// The air between the camera and a point dist metres away in direction dir (from the
// camera), at heights h0 (camera) and h1 (point) above the fog's base: rgb the light it
// scatters towards the camera (radiance), a what it lets through of the point.
fn air(dir: vec3<f32>, dist: f32, h0: f32, h1: f32) -> vec4<f32> {
    return air_of(dir, dist, h0, h1, 1.0);
}

fn rain_cell(ix: i32, iy: i32) -> f32 {
    let k = clamp(iy, 0, 15) * 16 + clamp(ix, 0, 15);
    return enh.rain[k / 4][k % 4];
}

fn rain_grid_at(g: vec2<f32>) -> f32 {
    let f = floor(g);
    let w = g - f;
    let ix = i32(f.x);
    let iy = i32(f.y);
    let a = mix(rain_cell(ix, iy), rain_cell(ix + 1, iy), w.x);
    let b = mix(rain_cell(ix, iy + 1), rain_cell(ix + 1, iy + 1), w.x);
    return mix(a, b, w.y);
}

fn rain_haze(dir: vec3<f32>, dist: f32, h0: f32) -> f32 {
    if (enh.debug.w <= 0.001) {
        return 0.0;
    }
    let reach = min(dist, 3200.0);
    var od = 0.0;
    for (var i = 0; i < 8; i = i + 1) {
        let t = (f32(i) + 0.5) / 8.0 * reach;
        let g = (dir.xy * t / 6400.0 + vec2<f32>(0.5)) * 16.0 - vec2<f32>(0.5);
        let up = h0 + dir.z * t;
        od = od + rain_grid_at(g) * (1.0 - smoothstep(600.0, 1500.0, up)) * reach / 8.0;
    }
    return od * 0.0025;
}

// The same with the clear air's share scaled (0 for the sky, whose table holds it already).
fn air_of(dir: vec3<f32>, dist: f32, h0: f32, h1: f32, clear: f32) -> vec4<f32> {
    // only the weather's fog: no haze on a clear day
    let tau = layer_depth(enh.fog.x, enh.fog.y, h0, h1, dist) + clear * rain_haze(dir, dist, h0);
    let t = exp(-tau);
    if (tau < 1e-5) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // clear air takes on the sky's own colour at the horizon in this direction (which
    // already holds the glow round the sun); a weather fog glows with the light around it
    // and brighter towards the sun
    let inscatter = enh.fog_color.rgb + enh.sun.rgb * hg_phase(dot(dir, camera.sun_dir.xyz), 0.55) * 0.9;
    return vec4<f32>(inscatter * (1.0 - t), t);
}
