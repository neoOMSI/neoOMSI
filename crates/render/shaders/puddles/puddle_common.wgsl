// The water normal shared by shading and scene reflections. The integer hash and the
// 1000 m period match shader.wgsl, keeping the rings fixed through floating-origin moves.
fn puddle_srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
}
fn puddle_srgb_decode(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(pow((x + 0.055) / 1.055, vec3<f32>(2.4)), x / 12.92, x <= vec3<f32>(0.04045));
}

fn puddle_hash(c: vec2<f32>) -> f32 {
    let cells = 8000.0;
    let w = c - cells * floor(c / cells);
    var v = vec2<u32>(w) * 1664525u + vec2<u32>(1013904223u);
    v.x = v.x + v.y * 1664525u;
    v.y = v.y + v.x * 1664525u;
    v = v ^ (v >> vec2<u32>(16u));
    v.x = v.x + v.y * 1664525u;
    v.y = v.y + v.x * 1664525u;
    v = v ^ (v >> vec2<u32>(16u));
    return f32(v.x >> 8u) / 16777215.0;
}

// xy: horizontal normal offset; z: the ring's strength, used for roughness as well.
fn puddle_ripple(pattern_xy: vec2<f32>, time: f32, rain: f32, coverage: f32) -> vec3<f32> {
    var bump = vec2<f32>(0.0);
    var strength = 0.0;
    for (var layer = 0; layer < 3; layer = layer + 1) {
        let fl = f32(layer);
        let p = pattern_xy * 5.0 + vec2<f32>(fl * 2.59, fl * 4.27);
        let cell = floor(p);
        let id = cell + vec2<f32>(fl * 31.0, fl * 17.0);
        let seed = puddle_hash(id);
        let jx = puddle_hash(id + vec2<f32>(7.7, 1.3));
        let jy = puddle_hash(id + vec2<f32>(3.1, 9.9));
        let centre = vec2<f32>(0.5) + (vec2<f32>(jx, jy) - vec2<f32>(0.5)) * 0.5;
        let phase = fract(time * (0.9 + seed * 0.8) + seed * 13.0);
        let local = fract(p) - centre;
        let r = length(local);
        let hit = step(1.0 - clamp(0.25 + 0.7 * rain, 0.0, 0.95), fract(seed * 31.7));
        let fade = (1.0 - phase) * (1.0 - phase) * rain * coverage * hit;
        var s = 0.0;
        for (var k = 0; k < 3; k = k + 1) {
            let rk = phase * 0.42 - f32(k) * 0.07;
            let ring = (1.0 - smoothstep(0.0, 0.03, abs(r - rk))) * step(0.0, rk) * (1.0 - 0.3 * f32(k));
            s = max(s, ring);
        }
        s = s * fade * 1.6;
        s = max(s, (1.0 - smoothstep(0.0, 0.06, r)) * smoothstep(0.85, 1.0, 1.0 - phase) * rain * coverage * hit);
        bump = bump + normalize(local + vec2<f32>(1e-5, 0.0)) * s * 0.12;
        strength = max(strength, s);
    }
    return vec3<f32>(bump, min(strength, 1.0));
}

fn puddle_wake(d: vec2<f32>, sh: f32, ch: f32, off: vec2<f32>, half: vec2<f32>, fwd: f32, time: f32) -> vec2<f32> {
    let speed = abs(fwd);
    let s = smoothstep(0.3, 4.0, speed);
    if (s <= 0.0) { return vec2<f32>(0.0); }
    let l = vec2<f32>(d.x * ch - d.y * sh, d.x * sh + d.y * ch) - off;
    let q = abs(l) - half;
    let e = length(max(q, vec2<f32>(0.0)));
    if (e > 6.0) { return vec2<f32>(0.0); }
    var o = sign(l) * max(q, vec2<f32>(0.0));
    if (dot(o, o) < 1e-6) { o = l; }
    o = normalize(o + vec2<f32>(0.0, 1e-5));
    let front = 0.35 + 0.65 * clamp(o.y * sign(fwd), 0.0, 1.0);
    let wave = cos(e * 4.0 - time * (2.0 + speed * 0.6));
    let amp = exp(-e * 0.55) * (0.55 + 0.45 * wave) * front * s;
    return vec2<f32>(o.x * ch + o.y * sh, -o.x * sh + o.y * ch) * amp * 0.45;
}
