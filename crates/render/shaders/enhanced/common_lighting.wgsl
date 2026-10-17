// Shared sky lighting and fog. Radiance is pre-exposed; 1 unit = 10,000 lux.
const SHADING_PROFILE_ENABLED: bool = true;

struct Enhanced {
    // x pre-exposure, y unlit level, z pre-exposed window radiance, w corona brightness
    exposure: vec4<f32>,
    // rgb irradiance of the sun after the clouds, w its angular radius (rad)
    sun: vec4<f32>,
    // irradiance from the sky and the ground: order-2 spherical harmonics, rgb
    sh: array<vec4<f32>, 9>,
    // rgb radiance of the ground, w the sky table's scale
    ground: vec4<f32>,
    // x base fog extinction, y height falloff, z base height relative to origin,
    // w clear-air extinction at ground; extinction and falloff are in 1/m
    fog: vec4<f32>,
    // rgb fog radiance, w reflection probe table scale
    fog_color: vec4<f32>,
    // x wet roads, y snow cover, z rain, w cloud cover in front of the sun
    weather: vec4<f32>,
    // x probe mip levels, y lamp-core illuminance, z cabin illuminance, w sun visibility
    lights: vec4<f32>,
    // rgb the sun disc's irradiance before the clouds, w frame time (s)
    sun_disc: vec4<f32>,
    // x debug term (OMSI_DEBUG_ENHANCED; 0 = full picture)
    debug: vec4<f32>,
    // xyz sky cube offset, w night level
    eye: vec4<f32>,
    // x LED brightness (0 = off), y LED `\S:n` mask mipmaps (0 = full resolution)
    led: vec4<f32>,
    // Up to three cloud layers, low to high: x base, y top (m), z cover, w shape (0 stratus..1 cumulus)
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

// Unscaled sky radiance towards d; multiply by enh.ground.w. The table uses sun-relative
// azimuth (0..π) and elevation, denser at the horizon.
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

// Optical depth from h0 to h1 along dist; extinction at height 0 is sigma, decaying by k/m.
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

// Air from camera to point: rgb scattered radiance, a transmitted light.
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

// Scale clear air separately; the sky table already includes it.
fn air_of(dir: vec3<f32>, dist: f32, h0: f32, h1: f32, clear: f32) -> vec4<f32> {
    // only the weather's fog: no haze on a clear day
    let tau = layer_depth(enh.fog.x, enh.fog.y, h0, h1, dist) + clear * rain_haze(dir, dist, h0);
    let t = exp(-tau);
    if (tau < 1e-5) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // Clear air takes the horizon colour; weather fog brightens towards the sun.
    let inscatter = enh.fog_color.rgb + enh.sun.rgb * hg_phase(dot(dir, camera.sun_dir.xyz), 0.55) * 0.9;
    return vec4<f32>(inscatter * (1.0 - t), t);
}
