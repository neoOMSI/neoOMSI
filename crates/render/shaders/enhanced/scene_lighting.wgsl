
@group(0) @binding(12) var t_probe: texture_cube<f32>;

fn d_ggx(nh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn v_smith(nv: f32, nl: f32, a: f32) -> f32 {
    let a2 = a * a;
    let gv = nl * sqrt(nv * nv * (1.0 - a2) + a2);
    let gl = nv * sqrt(nl * nl * (1.0 - a2) + a2);
    return 0.5 / max(gv + gl, 1e-5);
}

fn f_schlick(f0: vec3<f32>, c: f32) -> vec3<f32> {
    let f = pow(1.0 - clamp(c, 0.0, 1.0), 5.0);
    return f0 + (vec3<f32>(1.0) - f0) * f;
}

fn env_brdf(f0: vec3<f32>, rough: f32, nv: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = c0 * rough + c1;
    let a004 = min(r.x * r.x, pow(2.0, -9.28 * nv)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + vec3<f32>(ab.y);
}

fn ao_bounce(ao: f32, albedo: vec3<f32>) -> vec3<f32> {
    let a = 2.0404 * albedo - vec3<f32>(0.3324);
    let b = -4.7951 * albedo + vec3<f32>(0.6417);
    let c = 2.7552 * albedo + vec3<f32>(0.6903);
    return max(vec3<f32>(ao), ((a * ao + b) * ao + c) * ao);
}

fn to_cube(d: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(d.x, d.z, d.y);
}

fn rain_env_enhanced(d: vec3<f32>, lod: f32) -> vec3<f32> {
    let e = textureSampleLevel(t_probe, s_lin, to_cube(d), lod).rgb * enh.fog_color.w;
    let surround = enh.fog_color.rgb * (0.25 / 0.9);
    return mix(surround, e, smoothstep(-0.05, 0.35, d.z));
}

fn cs_hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

fn cs_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(cs_hash(i), cs_hash(i + vec2<f32>(1.0, 0.0)), u.x),
               mix(cs_hash(i + vec2<f32>(0.0, 1.0)), cs_hash(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

fn cs_fbm(p: vec2<f32>) -> f32 {
    return cs_noise(p) * 0.5 + cs_noise(p * 2.03 + 11.7) * 0.3 + cs_noise(p * 4.1 + 5.3) * 0.2;
}

fn cloud_sun_visibility(world: vec3<f32>) -> f32 {
    let sd = normalize(camera.sun_dir.xyz);
    if (sd.z < 0.05) {
        return 1.0;
    }
    let have = (enh.layers[0].z + enh.layers[1].z + enh.layers[2].z) > 0.01;
    var vis = 1.0;
    for (var li = 0; li < 3; li = li + 1) {
        var lay = enh.layers[li];
        if (!have) {
            if (li > 0) {
                break;
            }
            lay = vec4<f32>(1400.0, 4200.0, camera.clouds.x, 1.0);
        }
        if (lay.z <= 0.01 || lay.y <= lay.x) {
            continue;
        }
        let fl = f32(li);
        let h = lay.x + (lay.y - lay.x) * 0.3;
        let t = max(h - world.z, 0.0) / sd.z;
        let g = world.xy + camera.world_origin.zw + sd.xy * t;
        let drift = camera.clouds.yz * 70000.0 * (1.0 + fl) + vec2<f32>(5300.0, 2900.0) * fl;
        let period = mix(9000.0, 5000.0, lay.w);
        let n = cs_fbm((g + drift) / period);
        let cover = clamp(0.15 + lay.z * 0.75, 0.0, 1.0);
        let dens = smoothstep(0.9 - cover * 0.8, 1.0 - cover * 0.8 + 0.15, n);
        let closed = select(0.0, 1.0, li == 0) * smoothstep(0.85, 1.0, lay.z);
        vis = vis * (1.0 - 0.85 * max(dens, closed) * mix(0.6, 1.0, lay.w));
    }
    return clamp(vis, 0.0, 1.0);
}

fn sun_shadow_soft(world_in: vec3<f32>, n: vec3<f32>, thin: bool) -> f32 {
    if (camera.shadow.x < 0.5) {
        return 1.0;
    }
    let world = world_in + select(vec3<f32>(0.0), camera.sun_dir.xyz * 0.5, thin);
    let ndl = clamp(dot(n, camera.sun_dir.xyz), 0.0, 1.0);
    let texel = camera.shadow.y;
    let close = shadow_close(world, n, ndl, thin);
    if (close.y >= 0.999) {
        return close.x;
    }
    let near_lp = camera.light_view_proj * vec4<f32>(world + shadow_push_near(n, ndl), 1.0);
    let near_uv = vec2<f32>(near_lp.x * 0.5 + 0.5, 0.5 - near_lp.y * 0.5);
    let near_edge = max(abs(near_uv.x - 0.5), abs(near_uv.y - 0.5));
    let near_valid = near_edge < 0.48 && near_lp.z >= 0.0 && near_lp.z <= 1.0;
    var near_slope = shadow_receiver_slope(camera.light_view_proj, n);
    var far_slope = shadow_receiver_slope(camera.light_view_proj_far, n);
    if (thin) {
        near_slope = vec2<f32>(0.0);
        far_slope = vec2<f32>(0.0);
    }
    let radial = distance(world, camera.cam_pos.xyz);
    let near_weight = select(
        0.0,
        1.0 - smoothstep(camera.shadow.z * 0.62, camera.shadow.z * 0.92, radial),
        near_valid,
    );
    var near_value = 1.0;
    if (near_weight > 0.001) {
        near_value = shadow_pcf_near(near_uv, near_lp.z, near_slope, texel);
    }
    near_value = mix(near_value, close.x, close.y);
    if (near_weight >= 0.999) {
        return near_value;
    }
    let flp = camera.light_view_proj_far * vec4<f32>(world + shadow_push_far(n, ndl), 1.0);
    let fuv = vec2<f32>(flp.x * 0.5 + 0.5, 0.5 - flp.y * 0.5);
    let far_valid = fuv.x >= 0.0 && fuv.x <= 1.0 && fuv.y >= 0.0 && fuv.y <= 1.0 && flp.z >= 0.0 && flp.z <= 1.0;
    var far_safe = 1.0;
    if (far_valid) {
        let far_value = shadow_pcf_far(fuv, flp.z, far_slope, texel);
        let far_edge = max(abs(fuv.x - 0.5), abs(fuv.y - 0.5));
        far_safe = mix(1.0, far_value, clamp((0.5 - far_edge) * 12.0, 0.0, 1.0));
    }
    return mix(far_safe, near_value, near_weight);
}

fn sun_shadow_hard(world: vec3<f32>, n: vec3<f32>) -> f32 {
    if (camera.shadow.x < 0.5) {
        return 1.0;
    }
    let ndl = clamp(dot(n, camera.sun_dir.xyz), 0.0, 1.0);
    let range = camera.flags.w;
    if (range > 0.0 && distance(world, camera.cam_pos.xyz) < range * 0.75) {
        let lp = camera.light_view_proj_close * vec4<f32>(world + shadow_push_close(n, ndl), 1.0);
        let uv = vec2<f32>(lp.x * 0.5 + 0.5, 0.5 - lp.y * 0.5);
        if (max(abs(uv.x - 0.5), abs(uv.y - 0.5)) < 0.48 && lp.z >= 0.0 && lp.z <= 1.0) {
            let scale = select(1.0, camera.post.w, camera.post.w > 0.0);
            let a = vec2<f32>(uv.x * 0.5 * scale + 0.5, uv.y * scale);
            return textureSampleCompareLevel(t_shadow, s_shadow, a, lp.z - SHADOW_BIAS_CLOSE / SHADOW_DEPTH_RANGE);
        }
    }
    let lp = camera.light_view_proj * vec4<f32>(world + shadow_push_near(n, ndl), 1.0);
    let uv = vec2<f32>(lp.x * 0.5 + 0.5, 0.5 - lp.y * 0.5);
    if (max(abs(uv.x - 0.5), abs(uv.y - 0.5)) < 0.48 && lp.z >= 0.0 && lp.z <= 1.0) {
        return textureSampleCompareLevel(t_shadow, s_shadow, vec2<f32>(uv.x * 0.5, uv.y), lp.z - SHADOW_BIAS_NEAR / SHADOW_DEPTH_RANGE);
    }
    return 1.0;
}

struct Surface {
    albedo: vec3<f32>,
    f0: vec3<f32>,
    rough: f32,
};

const KIND_HEADLAMP: f32 = 199.0;
const KIND_HEADLAMP_MAIN: f32 = 299.0;
const OMNI_DIR_W: f32 = -1.5;

const HEAD_I: f32 = 20.5;
const HEAD_MAIN_PEAK: f32 = 3.5;

const BEAM_OK: u32 = 0u;
const BEAM_VERTICAL: u32 = 1u;
const BEAM_BEHIND: u32 = 2u;

struct BeamAngle {
    h: f32,
    v: f32,
    state: u32,
};

fn beam_angle(to_surface: vec3<f32>, f: vec3<f32>) -> BeamAngle {
    var r = cross(f, vec3<f32>(0.0, 0.0, 1.0));
    if (dot(r, r) < 1e-6) {
        return BeamAngle(0.0, 0.0, BEAM_VERTICAL);
    }
    r = normalize(r);
    let u = cross(r, f);
    let x = dot(to_surface, f);
    if (x <= 0.001) {
        return BeamAngle(0.0, 0.0, BEAM_BEHIND);
    }
    let y = dot(to_surface, r);
    let z = dot(to_surface, u);
    return BeamAngle(degrees(atan2(y, x)), degrees(atan2(z, sqrt(x * x + y * y))), BEAM_OK);
}

fn headlamp_profile(to_surface: vec3<f32>, f: vec3<f32>, cos_outer: f32, main_beam: bool) -> f32 {
    let a = beam_angle(to_surface, f);
    if (a.state == BEAM_VERTICAL) {
        return 1.0;
    }
    if (a.state == BEAM_BEHIND) {
        return 0.0;
    }
    let h = a.h;
    let v = a.v;
    let outer = max(degrees(acos(clamp(cos_outer, -1.0, 1.0))), 5.0);
    let off_axis = degrees(acos(clamp(dot(to_surface, f) / max(length(to_surface), 1e-4), -1.0, 1.0)));
    let edge = 1.0 - smoothstep(0.15 * outer, 1.45 * outer, off_axis);

    if (main_beam) {
        let core = exp(-0.5 * (h * h / 49.0 + v * v / 17.6));
        let halo = exp(-0.5 * (h * h / 484.0 + v * v / 81.0));
        return (HEAD_MAIN_PEAK * core + 0.5 * halo + 0.04) * edge;
    }

    let cut = smoothstep(-1.5, 8.0, h);
    let drop = cut - v;
    let soft = 1.4 + 0.18 * abs(h);
    let below = smoothstep(-soft, soft, drop - 0.04 * abs(h));
    let sigma = clamp(10.0 + 3.0 * max(drop, 0.0), 10.0, 45.0);
    let hh = (h - 2.0) / sigma;
    let hot = exp(-0.5 * hh * hh) * (0.06 + 0.94 * exp(-max(drop - 0.8, 0.0) / 6.0));
    let fore = 0.3 * (1.0 - smoothstep(14.0, 32.0, max(drop, 0.0)));
    let lit = max(hot, fore * exp(-0.5 * h * h / 900.0));
    return mix(0.03, lit, below) * edge;
}

fn light_falloff(dist2: f32, range: f32, core_in: f32) -> vec2<f32> {
    var core = core_in;
    if (core <= 0.0) {
        core = range * 0.125;
    }
    let q = dist2 / (range * range);
    let window = (1.0 - q * q) * (1.0 - q * q);
    let knee = core * core / sqrt(dist2 * dist2 + core * core * core * core);
    return vec2<f32>(knee * window, window);
}

fn headlamp_energy(l: PointLight, ld: vec3<f32>, dist: f32, dist2: f32, window: f32) -> f32 {
    let rel = headlamp_profile(-ld, l.dir.xyz, l.dir.w, l.extra.z >= KIND_HEADLAMP_MAIN);
    let e = HEAD_I * rel / max(dist2, 4.0) * window * smoothstep(1.0, 5.0, dist);
    return e * smoothstep(0.00015, 0.004, e);
}

fn cone_energy(l: PointLight, ld: vec3<f32>, e_in: f32) -> f32 {
    var e = e_in * smoothstep(l.dir.w, l.extra.x, dot(-ld, l.dir.xyz));
    let kind = l.extra.z;
    if (kind == 0.0) {
        return e;
    }
    let axis = max(-l.dir.z, 0.05);
    if (kind < 0.0) {
        let drop = abs(ld.z);
        return e * clamp(axis * axis / max(drop * drop, 1e-6), 1.0, -kind);
    }
    let drop = ld.z;
    let gain = clamp(axis * axis / max(drop * drop, 1e-6), 1.0, kind);
    e = e * mix(1.0, gain, smoothstep(-0.04, 0.0, drop));
    let fwd = -ld;
    let right = vec2<f32>(l.dir.y, -l.dir.x);
    let side = dot(fwd.xy / max(length(fwd.xy), 1e-4), right / max(length(right), 1e-4));
    let allowed = mix(-0.012, 0.11, smoothstep(0.0, 0.3, side));
    return e * mix(0.02, 1.0, 1.0 - smoothstep(allowed, allowed + 0.025, fwd.z));
}

fn lamp_light(p: vec3<f32>, n: vec3<f32>, v: vec3<f32>, sf: Surface, thin: bool) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let cell = camera.light_grid.z;
    let side = u32(camera.light_grid.w);
    if (cell <= 0.0 || side == 0u) {
        return sum;
    }
    let g = (p.xy - camera.light_grid.xy) / cell;
    let gx = i32(floor(g.x));
    let gy = i32(floor(g.y));
    if (gx < 0 || gy < 0 || gx >= i32(side) || gy >= i32(side)) {
        return sum;
    }
    let a = max(sf.rough * sf.rough, 0.3);
    let nv = max(dot(n, v), 1e-4);
    let base = (u32(gy) * side + u32(gx)) * CELL_CAP;
    for (var j = 0u; j < CELL_CAP; j = j + 1u) {
        let li = grid[base + j];
        if (li == 0xffffffffu) {
            break;
        }
        let l = lights[li];
        let d = l.pos.xyz - p;
        let dist2 = dot(d, d);
        let range = l.extra.w;
        if (dist2 >= range * range) {
            continue;
        }
        let dist = sqrt(dist2);
        let ld = d / max(dist, 1e-3);
        let fall = light_falloff(dist2, range, l.extra.y);
        let is_spot = l.dir.w > OMNI_DIR_W;

        var e = fall.x;
        if (is_spot && l.extra.z >= KIND_HEADLAMP) {
            e = headlamp_energy(l, ld, dist, dist2, fall.y);
        } else if (is_spot) {
            e = cone_energy(l, ld, e);
        } else {
            e = e * smoothstep(0.4, 1.5, dist);
        }

        let nl = dot(n, ld);
        if (e < 0.00005 || (!thin && nl <= 0.0)) {
            continue;
        }
        e = e * light_shadow(l, p + n * 0.08);
        if (e <= 0.0) {
            continue;
        }
        let irradiance = l.color.rgb * l.color.w * enh.lights.y * e;
        if (thin) {
            sum = sum + irradiance * (0.45 + 0.25 * nl) * sf.albedo / PI;
            continue;
        }
        let h = normalize(ld + v);
        let spec = d_ggx(max(dot(n, h), 0.0), a) * v_smith(nv, nl, a) * f_schlick(sf.f0, dot(v, h));
        sum = sum + irradiance * nl * (sf.albedo / PI + spec);
    }
    return sum;
}

const CAB_AMBIENT: f32 = 1.15;

fn led_lod(uv: vec2<f32>, texels: vec2<f32>) -> f32 {
    let dx = dpdx(uv) * texels;
    let dy = dpdy(uv) * texels;
    return max(0.5 * log2(max(dot(dx, dx), dot(dy, dy))), 0.0);
}

fn perturb_normal(n: vec3<f32>, p: vec3<f32>, uv: vec2<f32>, tn: vec3<f32>) -> vec3<f32> {
    let dp1 = dpdx(p);
    let dp2 = dpdy(p);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);
    let dp2perp = cross(dp2, n);
    let dp1perp = cross(n, dp1);
    let t = dp2perp * duv1.x + dp1perp * duv2.x;
    let b = dp2perp * duv1.y + dp1perp * duv2.y;
    let m = max(dot(t, t), dot(b, b));
    if (m < 1e-20) {
        return n;
    }
    let k = inverseSqrt(m);
    return safe_normal(t * k * tn.x + b * k * tn.y + n * max(tn.z, 0.05));
}

struct EnhancedOut {
    @location(0) color: vec4<f32>,
    @location(1) mask: vec4<f32>,
};

@fragment
fn fs_enhanced(in: FsIn) -> EnhancedOut {
    var puddle_weight = vec2<f32>(0.0);
    let c = shade_enhanced(in, &puddle_weight, false, camera.cam_pos.xyz);
    let screen = material.flags.x > 0.5;
    let led = select(0.0, step(0.5, c.a), material.emissive.w < -1.5);
    let night_lit = material.extra.w > 0.5 && material.extra.x < 0.5 && !screen && material.emissive.w > -0.5
        && (material.extra.w > 1.5 || camera.sun_color.w > 0.05) && c.a > 0.5;
    var out: EnhancedOut;
    out.color = c;
    let coverage = select(select(c.a, 1.0, screen), 0.0, in.params2.w > 1.5);
    let night_g = select(0.0, 0.75, night_lit);
    out.mask = vec4<f32>(select(0.0, 1.0, screen), max(max(led, night_g), puddle_weight.y * 0.49), puddle_weight.x, coverage);
    return out;
}

@fragment
fn fs_vanilla_reflections(in: FsIn) -> EnhancedOut {
    var weight = 0.0;
    let c = shade_vanilla(in, &weight, false);
    let coverage = select(c.a, 0.0, in.params2.w > 1.5);
    var out: EnhancedOut;
    out.color = c;
    out.mask = vec4<f32>(0.0, weight * 0.49, weight, coverage);
    return out;
}

fn shade_enhanced(in: FsIn, puddle_weight: ptr<function, vec2<f32>>, capture: bool, eye: vec3<f32>) -> vec4<f32> {
    if (material.emissive.w > 1.5) {
        let v = camera.cam_pos.xyz - in.world;
        let vn = normalize(v);
        let in_cab = inside_vehicle(camera.cam_pos.xyz) * near_player_vehicle(in.world) > 0.5;
        let g = rain_glass(in.world, in.uv - in.params.zw, in.normal, window_wetness(in), camera.post.y, in_cab, in.wipe_uv);
        if (g.cover <= 0.001 && g.mist <= 0.001) { return vec4<f32>(0.0); }
        let through = rain_through(g, vn);
        let valid = dot(through, through) > 1e-4;
        let seen = select(vec3<f32>(0.0), rain_behind(in.world, through, rain_env_enhanced(normalize(select(g.out, through, valid)), 2.0), 1.0 / max(enh.exposure.x, 1e-6), g.mist), valid);
        let mirrored = rain_env_enhanced(reflect(-vn, g.n), 1.0);
        let d = rain_light(g, vn, through, mirrored, seen, sh_irradiance(g.out) / PI * 0.9, enh.sun.rgb / PI);
        let aer = air(-normalize(v), fog_distance(in.world), camera.cam_pos.z - enh.fog.z, in.world.z - enh.fog.z);
        let near = 1.0 - smoothstep(5.0, 15.0, length(v));
        return vec4<f32>(d.rgb * enh.exposure.x * aer.a, d.a * near);
    }
    let terrain = material.extra.x > 0.5;
    var duv = tex_address(in.uv);
    if (terrain) {
        duv = in.uv * material.extra.z;
    }
    let pic_lod = led_lod(duv, vec2<f32>(textureDimensions(t_diffuse)));
    let led_pic = material.emissive.w < -1.5 && material.emissive.w > -2.5 && enh.led.y < pic_lod;
    var tex = diffuse_border(textureSample(t_diffuse, s_diffuse, duv), duv);
    if (led_pic) {
        tex = diffuse_border(textureSampleLevel(t_diffuse, s_diffuse, duv, enh.led.y), duv);
    }
    let diffuse_a = tex.a;
    let buv = tex_address(in.uv - in.params.zw);
    let msk_lod = led_lod(buv, vec2<f32>(textureDimensions(t_trans)));
    if (terrain && material.extra.y > 0.0) {
        let det = textureSample(t_light, s_diffuse, in.uv * material.extra.y);
        tex = vec4<f32>(clamp(tex.rgb * det.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), tex.a);
    }
    if (material.params.z > 0.5) {
        var tm = sample_transmap(buv);
        if (material.emissive.w < -1.5 && material.emissive.w > -2.5 && enh.led.y < msk_lod) {
            tm = textureSampleLevel(t_trans, s_diffuse, buv, enh.led.y);
        }
        tm = diffuse_border(tm, in.uv - in.params.zw);
        tex.a = select(1.0, tm.a, material.params.w > 0.5);
        if (terrain && material.params.x > 1.5) {
            tex.a = smoothstep(0.32, 0.68, tex.a);
        }
    }
    let mode = material.params.x;
    if ((ALPHA_TEST || capture) && mode > 0.5 && mode < 1.5) {
        if (ALPHA_TO_COVERAGE) {
            let aa = max(fwidth(tex.a) * 0.5, 1.0 / 255.0);
            if (tex.a < 0.5 - aa) {
                discard;
            }
            tex.a = smoothstep(0.5 - aa, 0.5 + aa, tex.a);
        } else if (tex.a < 0.5) {
            discard;
        }
    }
    var alpha = tex.a * material.color.a;
    if (mode < 0.5) {
        alpha = 1.0;
    }
    alpha = alpha * clamp(window_wetness(in), 0.0, 1.0);
    let pre = enh.exposure.x;
    let to_cam = eye - in.world;
    let dist = length(to_cam);
    let v = to_cam / max(dist, 1e-4);
    let h_cam = camera.cam_pos.z - enh.fog.z;
    let h_pt = in.world.z - enh.fog.z;
    let aer = air(-v, fog_distance(in.world), h_cam, h_pt);
    if (material.params.y > 0.5) {
        let t = tex.rgb * material.color.rgb;
        let peak = max(t.r, max(t.g, t.b));
        let k = min(1.0, 0.76 / max(peak, 1e-3));
        let tk = t * k;
        let lift = select(enh.exposure.y, min(enh.exposure.y, 1.0), material.params.y < 0.95);
        let screen_dim = select(1.0, 0.55, material.flags.x > 0.5);
        let c = (tk + 0.04 * smoothstep(vec3<f32>(0.0), vec3<f32>(0.08), tk)) * lift * screen_dim;
        return vec4<f32>(c * aer.a + aer.rgb * pre, alpha);
    }
    let outside = weather_outside_n(in.world, safe_normal(in.normal), terrain, in.params2.w);
    var n = safe_normal(in.normal);
    let is_water = material.ambient.w > 1.5;
    if (is_water) {
        n = normalize(n + vec3<f32>(water_ripple(world_pattern_xy(in.world), camera.post.y) * clamp(1.0 - dist / 250.0, 0.2, 1.0), 0.0));
    }
    let thin = !terrain && mode > 0.5 && mode < 1.5;
    let has_env = material.params2.y > 0.0;
    let glass = !terrain && mode > 1.5 && material.bump.z > 0.5 &&
        (has_env || material.params.z > 0.5 || material.emissive.w > 0.5);
    let painted_transmap = material.params.z > 0.5 && !glass;
    let reflective_env = has_env && !painted_transmap && !thin && !is_water;
    var geo_n = n;
    if (glass && dot(n, v) < 0.0) {
        n = -n;
        geo_n = n;
    }
    if (reflective_env && material.bump.y > 0.5) {
        let slope = bump_offset(duv);
        let dp1 = dpdx(in.world);
        let dp2 = dpdy(in.world);
        let du1 = dpdx(duv);
        let du2 = dpdy(duv);
        let dp2p = cross(dp2, n);
        let dp1p = cross(n, dp1);
        let t = dp2p * du1.x + dp1p * du2.x;
        let b = dp2p * du1.y + dp1p * du2.y;
        let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-12));
        n = normalize(n - (t * inv * slope.x + b * inv * slope.y) * 0.6);
    }
    var albedo = tex.rgb * material.color.rgb;
    var detail_factor = 1.0;
    if (camera.flags.x > 0.5 && (terrain || in.params2.w > 0.5)) {
        let k = clamp(1.0 - (dist - 25.0) / 120.0, 0.0, 1.0);
        let pattern_xy = world_pattern_xy(in.world);
        detail_factor = 1.0 + (detail_noise(pattern_xy) - 0.5) * 0.42 * k;
        albedo = albedo * detail_factor;
    }
    var refl = 0.0;
    if (reflective_env) {
        refl = clamp(min(material.params2.y, 1.0) * reflection_mask(duv, diffuse_a), 0.0, 1.0);
    }
    var metal = 0.0;
    var f0 = vec3<f32>(0.04);
    var rough = 0.8;
    if (terrain) {
        rough = 0.92;
        f0 = vec3<f32>(0.03);
    } else if (glass) {
        rough = 0.04;
        f0 = vec3<f32>(clamp(0.04 + 0.02 * min(material.params2.y, 1.0), 0.04, 0.06));
    } else if (reflective_env) {
        let masked = (u32(material.params2.w + 0.5) & 1u) != 0u;
        let metal_ok = (u32(material.params2.w + 0.5) & 4u) != 0u;
        metal = select(0.0, smoothstep(0.3, 0.85, refl), masked || metal_ok);
        f0 = mix(vec3<f32>(clamp(refl, 0.02, 0.08)), mix(albedo, vec3<f32>(1.0), 0.4) * refl, metal);
        rough = mix(max(0.3 - 0.12 * smoothstep(0.0, 0.25, refl), select(0.22, 0.0, masked || metal_ok)), 0.14, metal);
    } else if (!thin && material.specular.w > 0.0 && dot(material.specular.rgb, vec3<f32>(1.0)) > 0.05) {
        rough = clamp(sqrt(sqrt(2.0 / (material.specular.w + 2.0))), 0.4, 0.9);
    } else if (thin) {
        rough = 0.7;
    }
    if (is_water) {
        f0 = vec3<f32>(0.02);
        rough = 0.06;
        metal = 0.0;
    }
    var pbr_ao = 1.0;
    if (material.pbr.x > 0.5) {
        var tn = textureSample(t_pbr_normal, s_diffuse, duv).xyz * 2.0 - vec3<f32>(1.0);
        if (material.pbr.x > 1.5) {
            tn.y = -tn.y;
        }
        n = perturb_normal(n, in.world, duv, tn);
    }
    if (material.pbr.y + material.pbr.z + material.pbr.w > 0.5) {
        let orm = textureSample(t_pbr_orm, s_diffuse, duv).rgb;
        if (material.pbr.y > 0.5) {
            pbr_ao = orm.r;
        }
        if (material.pbr.z > 0.5) {
            rough = clamp(orm.g, 0.03, 1.0);
        }
        if (material.pbr.w > 0.5) {
            metal = orm.b;
            f0 = mix(vec3<f32>(0.04), albedo, metal);
        }
    }
    let dry_snow = 1.0 - clamp(enh.weather.y, 0.0, 1.0);
    let wet_road = camera.shadow.w * material.params2.z * outside * dry_snow;
    let wet_any = camera.shadow.w * outside * select(0.35, 0.0, glass || material.emissive.w < -1.5) * dry_snow;
    var puddle = 0.0;
    if (wet_road > 0.0) {
        albedo = albedo * mix(1.0, 0.5, wet_road);
        rough = mix(rough, 0.12, wet_road);
        f0 = mix(f0, vec3<f32>(0.02), wet_road);
        let pattern_xy = world_pattern_xy(in.world);
        let pn = vnoise_f(pattern_xy, 0.22, vec2<f32>(17.3, -9.1)) * 0.65 + vnoise_f(pattern_xy, 0.9, vec2<f32>(-4.0, 8.0)) * 0.35;
        let puddle_t = 1.0 - wet_road * PUDDLE_SPREAD;
        puddle = smoothstep(puddle_t - 0.06, puddle_t + 0.06, pn) * smoothstep(0.75, 0.95, n.z);
        if (puddle > 0.001) {
            let raining = enh.weather.z * (1.0 - enh.weather.y);
            let ripple = puddle_ripple(pattern_xy, camera.post.y, raining, puddle);
            var wake = vec2<f32>(0.0);
            if (camera.inside_c.w > 0.5) {
                let fwd = dot(camera.wind.xy, vec2<f32>(camera.inside_a.w, camera.inside_b.x));
                wake = puddle_wake((in.world - camera.inside_a.xyz).xy, camera.inside_a.w, camera.inside_b.x,
                    camera.inside_c.xy, camera.inside_b.yz, fwd, camera.post.y);
            }
            albedo = albedo * (1.0 - 0.68 * puddle) / mix(1.0, detail_factor, puddle * 0.8);
            rough = clamp(mix(rough, 0.03, puddle) - ripple.z * 0.12, 0.02, 1.0);
            f0 = mix(f0, vec3<f32>(enh.debug.y), puddle);
            n = normalize(mix(n, geo_n, puddle) + vec3<f32>(ripple.xy + wake * puddle, 0.0));
        }
    } else if (wet_any > 0.0 && !terrain) {
        rough = mix(rough, rough * 0.6, wet_any);
    }
    let snow = enh.weather.y * outside * select(1.0, 0.0, in.params2.w > 1.5 || material.ambient.w > 0.5);
    if (snow > 0.0) {
        let up = clamp(n.z, 0.0, 1.0);
        let ground = select(0.0, 1.0, terrain || material.params2.z > 0.0);
        let cover = snow * clamp(max(ground, smoothstep(0.78, 0.95, up) * 0.8), 0.0, 1.0) * (0.55 + 0.35 * tex.a);
        albedo = mix(albedo, vec3<f32>(0.82, 0.84, 0.88), cover);
        rough = mix(rough, 0.95, cover);
        f0 = mix(f0, vec3<f32>(0.02), cover);
        metal = metal * (1.0 - cover);
    }
    if (reflective_env && !glass) {
        let dndx = dpdx(n);
        let dndy = dpdy(n);
        let spread = min((dot(dndx, dndx) + dot(dndy, dndy)) * 2.0, 0.4);
        rough = sqrt(min(rough * rough + spread, 1.0));
    }
    var sf: Surface;
    sf.albedo = albedo * (1.0 - metal);
    sf.f0 = f0;
    sf.rough = rough;
    let nv = clamp(dot(n, v), 1e-4, 1.0);
    let s = camera.sun_dir.xyz;
    let nl = dot(n, s);
    var direct = vec3<f32>(0.0);
    if (enh.lights.w > 0.0 && max(enh.sun.r, enh.sun.g) > 1e-5) {
        var shadow = 0.0;
        if (glass && nl > 0.0) {
            shadow = sun_shadow_hard(in.world, n);
        } else if (nl > 0.0 || thin) {
            shadow = sun_shadow_soft(in.world, n, thin);
        }
        let e_sun = enh.sun.rgb * shadow * cloud_sun_visibility(in.world);
        if (thin) {
            direct = e_sun * (0.3 + 0.4 * max(nl, 0.0) + 0.1 * max(-nl, 0.0)) * sf.albedo / PI;
        } else if (nl > 0.0) {
            let a = max(rough * rough, 0.012);
            let h = normalize(s + v);
            let spec = d_ggx(max(dot(n, h), 0.0), a) * v_smith(nv, nl, a) * f_schlick(f0, dot(v, h));
            direct = e_sun * nl * (sf.albedo / PI * (vec3<f32>(1.0) - f_schlick(f0, nl)) + spec * select(1.0, 0.12, glass));
        }
    }
    var ao = 1.0;
    if (camera.clouds.w > 0.5 && !capture) {
        ao = ao_at(in.clip.xy, in.world);
    }
    if (glass || mode > 1.5) {
        ao = 1.0;
    }
    ao = ao * pbr_ao;
    let fr = f_schlick(f0, nv);
    let cabin_mesh = in.params.y > 1.5;
    let in_cab = max(1.0 - outside, select(0.0, 1.0, cabin_mesh));
    ao = mix(ao, 1.0 - (1.0 - ao) * 0.35, in_cab);
    let avg_e = enh.fog_color.rgb * (PI / 0.9);
    let cab_e = vec3<f32>(dot(avg_e, vec3<f32>(0.2126, 0.7152, 0.0722))) * vec3<f32>(1.0, 0.98, 0.95);
    let e_amb = mix(sh_irradiance(n), cab_e * CAB_AMBIENT, 0.6 * in_cab) * ao_bounce(ao, sf.albedo);
    var ambient = e_amb * sf.albedo / PI * (vec3<f32>(1.0) - fr * (1.0 - rough));
    var r = reflect(-v, n);
    let below = dot(r, geo_n);
    if (below < 0.0) {
        r = normalize(r - geo_n * below * 1.02);
    }
    let lod = rough * (enh.lights.x - 1.0);
    var env = textureSampleLevel(t_probe, s_lin, to_cube(r), lod).rgb * enh.fog_color.w;
    let surround = enh.fog_color.rgb * (0.25 / 0.9);
    let open = smoothstep(-0.05, 0.35, r.z);
    env = mix(surround, env, mix(open, 1.0, select(0.25, 0.5, reflective_env)));
    let cab_view = max(in_cab, near_player_vehicle(in.world) * inside_vehicle(camera.cam_pos.xyz));
    let cabin_reflection = select(0.85, 0.0, glass);
    env = mix(env, cab_e * 0.35 / PI, cabin_reflection * cab_view);
    let own_pane = select(0.0, near_player_vehicle(in.world) * inside_vehicle(camera.cam_pos.xyz), glass);
    if (reflective_env) {
        let az = atan2(r.y, r.x);
        var env_uv = vec2<f32>(0.5 + 0.3 * sin(az), 0.5 + 0.45 * clamp(r.z, -1.0, 1.0));
        if (material.bump.y > 0.5) {
            env_uv = env_uv + bump_offset(duv);
        }
        let photo = textureSampleBias(t_env, s_diffuse, env_uv, max(rough * 16.0, select(2.0, 0.0, glass))).rgb;
        let photo_avg = textureSampleLevel(t_env, s_diffuse, vec2<f32>(0.5, 0.5), 12.0).rgb;
        let lum_avg = max(dot(photo_avg, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.02);
        let ratio = clamp(photo / lum_avg, vec3<f32>(0.35), vec3<f32>(2.0));
        let band = 1.0 - smoothstep(0.25, 0.7, abs(r.z));
        let sharpness = 1.0 - smoothstep(0.05, 0.25, rough);
        let outside_env = 1.0 - cab_view;
        let clear_air = exp(-enh.fog.x * 150.0);
        env = env * mix(vec3<f32>(1.0), ratio, band * 0.65 * mix(0.25, 1.0, sharpness) * outside_env * clear_air * enh.debug.z);
    }
    let spec_occ = clamp(pow(nv + ao, exp2(-16.0 * rough - 1.0)) - 1.0 + ao, 0.0, 1.0);
    let pbr_reflects = material.pbr.z > 0.5 || material.pbr.w > 0.5;
    let reflects = reflective_env || pbr_reflects || is_water || wet_road > 0.0;
    var reflection = select(vec3<f32>(0.0), env * env_brdf(f0, rough, nv) * spec_occ * select(1.0, wet_road, !(reflective_env || glass || pbr_reflects || is_water)), reflects);
    if (!reflects) {
        ambient = e_amb * sf.albedo / PI;
    }
    if (glass) {
        reflection = reflection * 0.20 * (1.0 - 0.85 * own_pane);
    }
    let cabin_light = interior_lamps(in.world, n, in.params2.z);
    let saloon_lit = select(0.0, clamp(max(cabin_light.r, max(cabin_light.g, cabin_light.b)) * 4.0, 0.0, 1.0), in.params2.z >= 1.0);
    let lamps = lamp_light(in.world, n, v, sf, thin) * select(1.0, 0.0, material.params.y > 0.2 && material.params.y < 0.3) * (1.0 - saloon_lit) * (1.0 + 0.9 * wet_road);
    let cabin = sf.albedo * cabin_light * mix(1.0, ao, 0.85);
    var rgb = (direct + ambient + lamps) * pre + cabin;
    var emit = tex.rgb * material.emissive.rgb * max(enh.exposure.z * 2.0, 0.8);
    if (material.extra.w > 0.5) {
        let nuv = select(buv, vec2<f32>(in.uv.x, 1.0 - in.uv.y), terrain);
        let switched = material.extra.w > 1.5;
        let night = select(camera.sun_color.w, 1.0, switched);
        let nm = sample_nightmap(nuv).rgb * night * select(clamp(in.params2.y, 0.0, 1.0), 1.0, switched);
        if (terrain) {
            rgb = rgb + sf.albedo / PI * nm * enh.lights.y * 3.0 * pre;
        } else {
            emit = emit + nm * camera.tune.x * select(enh.exposure.z, max(enh.exposure.z * 2.0, 0.8), switched);
        }
    }
    if (material.params2.x > 0.5 && !terrain) {
        let lm = textureSample(t_light, s_diffuse, buv).rgb;
        let night = clamp(camera.sun_color.w, 0.0, 1.0);
        let left = (vec3<f32>(1.0) - clamp(cabin_light, vec3<f32>(0.0), vec3<f32>(1.0))) * (0.12 + 0.88 * night);
        emit = emit + tex.rgb * lm * left * clamp(in.params2.x, 0.0, 1.0) * camera.tune.y * max(enh.exposure.z * 2.0, 0.6);
    }
    if (material.emissive.w < -2.5) {
        emit = emit + tex.rgb * (0.35 + 0.25 * enh.led.x) * enh.led.z * max(enh.exposure.z * 2.0, 0.8);
    } else if (material.emissive.w < -1.5) {
        let lm_gate = select(1.0, clamp(in.params2.x, 0.0, 1.0), material.params2.x > 0.5);
        emit = emit + tex.rgb * enh.led.x * alpha * lm_gate * max(enh.exposure.z * 2.0, 0.8);
    } else if (material.emissive.w < -0.5) {
        emit = emit + tex.rgb * 0.2 * enh.led.w * max(enh.exposure.z * 2.0, 0.8);
    } else if (material.flags.x > 0.5 && material.emissive.w > -0.5 && material.emissive.w < 0.5) {
        emit = emit + tex.rgb * 0.2 * max(enh.led.w - 1.0, 0.0) * max(enh.exposure.z * 2.0, 0.8);
    }
    rgb = rgb + emit;
    if (enh.debug.x > 0.5) {
        let dm = i32(enh.debug.x);
        var dc = vec3<f32>(0.0);
        switch dm {
            case 1: { dc = vec3<f32>(sun_shadow_soft(in.world, n, thin)); }
            case 2: { dc = vec3<f32>(ao); }
            case 3: { dc = n * 0.5 + vec3<f32>(0.5); }
            case 4: { dc = vec3<f32>(aer.a); }
            case 5: { dc = e_amb * pre * 0.1; }
            case 6: { dc = reflection * pre; }
            case 7: { dc = sf.albedo; }
            case 8: { dc = direct * pre; }
            case 9: { dc = aer.rgb * pre; }
            case 11: { dc = vec3<f32>(fract(dist / 10.0), dist / 1000.0, in.clip.z * 100.0); }
            case 12: { dc = vec3<f32>(mode * 0.5, f32(terrain), in.params2.w); }
            case 13: { dc = vec3<f32>(in_cab, ao, spec_occ); }
            case 14: { dc = (ambient + direct) * pre; }
            case 15: { dc = lamps * pre; }
            case 16: { dc = emit; }
            case 17: { dc = vec3<f32>(rough, f0.g * 10.0, metal); }
            default: { dc = vec3<f32>(alpha, f32(glass), f32(has_env)); }
        }
        return vec4<f32>(dc, 1.0);
    }
    if (glass) {
        let cover = smoothstep(0.0, 0.05, alpha);
        let refl_rgb = reflection * pre * cover;
        let rl = dot(refl_rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let reflected_opacity = select(0.0, clamp(rl * 0.15 + fr.g, 0.0, 0.15), reflective_env);
        let a2 = clamp(alpha + (1.0 - alpha) * reflected_opacity * (1.0 - 0.85 * own_pane) * cover, alpha, 1.0);
        let c = (rgb * alpha + refl_rgb) / max(a2, 1e-3);
        return vec4<f32>(c * aer.a + aer.rgb * pre, a2);
    }
    rgb = rgb + reflection * pre;
    if (!glass && !reflective_env) {
        let weight = clamp(puddle * env_brdf(f0, rough, nv).g
            * select(wet_road, 1.0, pbr_reflects) * aer.a, 0.0, 1.0);
        *puddle_weight = vec2<f32>(weight, weight * spec_occ);
    }
    let a_out = select(alpha, clamp(alpha + (1.0 - alpha) * fr.g, alpha, 1.0), is_water);
    return vec4<f32>(rgb * aer.a + aer.rgb * pre, a_out);
}

fn water_ripple(p: vec2<f32>, t: f32) -> vec2<f32> {
    var g = vec2<f32>(0.0);
    let dirs = array<vec2<f32>, 4>(vec2<f32>(0.8, 0.6), vec2<f32>(-0.45, 0.89), vec2<f32>(0.96, -0.28), vec2<f32>(-0.7, -0.71));
    let lens = array<f32, 4>(4.7, 2.9, 1.9, 1.3);
    for (var i = 0; i < 4; i = i + 1) {
        let k = 6.2831853 / lens[i];
        let w = sqrt(9.81 * k);
        g = g + dirs[i] * cos(dot(p, dirs[i]) * k - w * t) * 0.045;
    }
    return g;
}

@fragment
fn fs_puddle_world(input: FsIn) -> @location(0) vec4<f32> {
    if (material.emissive.w > 1.5) { discard; }
    if (camera.post.x > 0.5) {
        var unused = vec2<f32>(0.0);
        return shade_enhanced(input, &unused, true, camera.cam_pos.xyz);
    }
    var unused = 0.0;
    return shade_vanilla(input, &unused, true);
}

@fragment
fn fs_puddle_vehicle(input: FsIn) -> @location(0) vec4<f32> {
    let plane = vehicle_reflection.plane;
    let height = dot(plane.xyz, input.world) - plane.w;
    if (height < 0.0 || material.emissive.w > 1.5) { discard; }
    var unused = vec2<f32>(0.0);
    let eye = camera.cam_pos.xyz - 2.0 * plane.xyz * (dot(plane.xyz, camera.cam_pos.xyz) - plane.w);
    return shade_enhanced(input, &unused, true, eye);
}

@fragment
fn fs_puddle_chassis() -> @location(0) vec4<f32> {
    return vec4<f32>(sh_irradiance(vec3<f32>(0.0, 0.0, -1.0)) * enh.exposure.x * 0.025, 1.0);
}
