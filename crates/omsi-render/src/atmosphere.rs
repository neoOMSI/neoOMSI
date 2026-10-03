//! The enhanced renderer's light: a physically based atmosphere - Rayleigh and Mie single
//! scattering over a spherical Earth with ozone absorption and an isotropic estimate of
//! the higher orders - evaluated on the CPU whenever the sun or the weather has moved on.
//! It yields the sun's colour and strength at the ground, the sky's radiance as a small
//! table the sky dome and the reflection probe read, the light the sky and the ground
//! throw on a surface of any orientation (spherical harmonics), and the exposure that
//! suits the whole of it.
//!
//! Units: 1 = 10 000 lux of irradiance (the noon sun is about 10, a street under its lamps
//! about 0.002) and 1 per steradian of radiance.

use glam::Vec3;

/// The sky table: azimuth from the sun (0..π, the sky is symmetric about the sun's
/// vertical) by elevation (-90°..90°, denser at the horizon).
pub const SKY_LUT_W: u32 = 32;
pub const SKY_LUT_H: u32 = 48;

const EARTH_R: f32 = 6_360_000.0;
const ATMO_R: f32 = 6_420_000.0;
/// Scattering coefficients at sea level (1/m) for red, green and blue.
const RAYLEIGH: Vec3 = Vec3::new(5.802e-6, 13.558e-6, 33.1e-6);
const RAYLEIGH_H: f32 = 8000.0;
/// Aerosol scattering of a clear day in a central European city (an optical depth of
/// about 0.05); extinction is a tenth more (absorption).
const MIE: f32 = 4.0e-5;
const MIE_EXT: f32 = 1.11;
const MIE_H: f32 = 1200.0;
const MIE_G: f32 = 0.76;
const OZONE: Vec3 = Vec3::new(0.650e-6, 1.881e-6, 0.085e-6);
/// The eye height the sky is seen from (m): the horizon dips a little below zero.
const OBSERVER_H: f32 = 250.0;
/// The sun's irradiance above the atmosphere.
const SUN_E0: f32 = 12.5;
/// How much the higher scattering orders add, relative to the first.
const MULTI: f32 = 2.0;
/// The share of the aerosols' scattered light that goes round again.
const MULTI_MIE: f32 = 0.2;
/// A night sky's own radiance: a city's sky glow with some moonlight (about 1.4 lux).
const NIGHT_SKY: Vec3 = Vec3::new(3.6e-5, 4.0e-5, 5.4e-5);
/// The light a lit city keeps up around the viewer at night (street lamps, windows) as far
/// as the exposure is concerned.
const ARTIFICIAL: f32 = 0.0015;
/// How much of the sky at the horizon houses and trees hide (for the ambient light), up to
/// which elevation (degrees), and how much light they throw back.
const SURROUND_MAX: f32 = 0.7;
const SURROUND_ELEVATION: f32 = 18.0;
const SURROUND_ALBEDO: f32 = 0.25;
/// How much the walls a low sun shines on count towards the exposure.
const LOW_SUN_WALLS: f32 = 0.3;
/// How much of the sky light's colour the eye discounts in the shade.
const SHADE_ADAPTATION: f32 = 0.3;
/// Irradiance of a sunlit and sky-lit horizontal surface at noon, the exposure reference.
const DAY_REFERENCE: f32 = 11.0;

/// What the sky is made of at a moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyInput {
    /// Towards the sun, world space (x east, y north, z up).
    pub sun_dir: Vec3,
    /// How much of the direct sun the clouds let through (0..1).
    pub sun_visibility: f32,
    /// A closed cloud cover turns the sky into an even grey dome (0..1).
    pub overcast: f32,
    /// Aerosol amount: 1 a clear day, more for haze, mist and rain.
    pub haze: f32,
    /// Rain: the sky light drops under a thick, wet cover (0..1).
    pub rain: f32,
    /// Albedo of the ground the light comes back from (snow is bright).
    pub ground_albedo: f32,
    /// envir.cfg's light colours relative to the stock ones: sun (A), sky (B), ambient (C).
    pub tint: [Vec3; 3],
    pub night_light: f32,
}

impl Default for SkyInput {
    fn default() -> Self {
        Self {
            sun_dir: Vec3::new(0.3, 0.2, 0.9).normalize(),
            sun_visibility: 1.0,
            overcast: 0.0,
            haze: 1.0,
            rain: 0.0,
            ground_albedo: 0.2,
            tint: [Vec3::ONE; 3],
            night_light: 1.0,
        }
    }
}

/// The light of the sky for one `SkyInput`.
#[derive(Debug, Clone)]
pub struct SkyState {
    pub input: SkyInput,
    /// Irradiance of the direct sun on a surface facing it, after the clouds.
    pub sun: Vec3,
    /// The sun disc's irradiance before the clouds (its colour and strength in the sky).
    pub sun_disc: Vec3,
    /// Sky radiance per table cell divided by `lut_scale`, RGBA (alpha unused).
    pub lut: Vec<[f32; 4]>,
    pub lut_scale: f32,
    /// Irradiance from the sky and the ground as order-2 spherical harmonics, already
    /// convolved with the cosine lobe: E(n) = Σ sh[i] · Y_i(n).
    pub sh: [Vec3; 9],
    /// Irradiance of a horizontal surface from the sky alone.
    pub sky_horizontal: Vec3,
    /// Radiance of the (distant) ground.
    pub ground: Vec3,
    /// Pre-exposure: scene radiance times this is what the picture holds (mid grey in the
    /// day's light comes out at about 0.18).
    pub exposure: f32,
}

/// Optical depth per unit coefficient (Rayleigh, Mie, ozone) along a ray from altitude h
/// in the direction with cosine of the zenith angle mu, to the top of the atmosphere;
/// huge where the ray meets the ground (no sunlight arrives along it).
struct DepthTable {
    rows: usize,
    cols: usize,
    depth: Vec<[f32; 3]>,
}

const H_TOP: f32 = ATMO_R - EARTH_R;

impl DepthTable {
    fn build() -> DepthTable {
        let (rows, cols) = (32usize, 128usize);
        let mut depth = Vec::with_capacity(rows * cols);
        for i in 0..rows {
            let h = Self::row_height(i, rows);
            for j in 0..cols {
                let mu = -1.0 + 2.0 * j as f32 / (cols - 1) as f32;
                depth.push(Self::integrate(h, mu));
            }
        }
        DepthTable { rows, cols, depth }
    }

    fn row_height(i: usize, rows: usize) -> f32 {
        let x = i as f32 / (rows - 1) as f32;
        x * x * H_TOP
    }

    fn integrate(h: f32, mu: f32) -> [f32; 3] {
        let r0 = EARTH_R + h;
        if ray_hits_ground(r0, mu).is_some() {
            return [1e9, 1e9, 1e9];
        }
        let t_max = ray_exit(r0, mu, ATMO_R);
        let n = 40;
        let mut acc = [0.0f32; 3];
        for k in 0..n {
            // denser near the start, where the air is thick
            let a = k as f32 / n as f32;
            let b = (k + 1) as f32 / n as f32;
            let (t0, t1) = (a * a * t_max, b * b * t_max);
            let t = 0.5 * (t0 + t1);
            let hh = altitude(r0, mu, t);
            let d = densities(hh);
            let dt = t1 - t0;
            acc[0] += d[0] * dt;
            acc[1] += d[1] * dt;
            acc[2] += d[2] * dt;
        }
        acc
    }

    fn lookup(&self, h: f32, mu: f32) -> [f32; 3] {
        let fr = (h.clamp(0.0, H_TOP) / H_TOP).sqrt() * (self.rows - 1) as f32;
        let fc = ((mu.clamp(-1.0, 1.0) + 1.0) * 0.5) * (self.cols - 1) as f32;
        let (r0, c0) = (fr.floor() as usize, fc.floor() as usize);
        let (r1, c1) = ((r0 + 1).min(self.rows - 1), (c0 + 1).min(self.cols - 1));
        let (tr, tc) = (fr - r0 as f32, fc - c0 as f32);
        let at = |r: usize, c: usize| self.depth[r * self.cols + c];
        let mut out = [0.0f32; 3];
        for (k, o) in out.iter_mut().enumerate() {
            let a = at(r0, c0)[k] * (1.0 - tc) + at(r0, c1)[k] * tc;
            let b = at(r1, c0)[k] * (1.0 - tc) + at(r1, c1)[k] * tc;
            *o = a * (1.0 - tr) + b * tr;
        }
        out
    }
}

fn depth_table() -> &'static DepthTable {
    static TABLE: std::sync::OnceLock<DepthTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(DepthTable::build)
}

/// Air, aerosol and ozone density (relative to sea level) at altitude h.
fn densities(h: f32) -> [f32; 3] {
    let ozone = (1.0 - (h - 25_000.0).abs() / 15_000.0).max(0.0);
    [(-h / RAYLEIGH_H).exp(), (-h / MIE_H).exp(), ozone]
}

/// Altitude after `t` metres from radius r0 along a ray with zenith cosine mu.
fn altitude(r0: f32, mu: f32, t: f32) -> f32 {
    (r0 * r0 + t * t + 2.0 * r0 * t * mu).max(0.0).sqrt() - EARTH_R
}

/// Distance to where the ray leaves the sphere of radius `radius` (the origin is inside).
fn ray_exit(r0: f32, mu: f32, radius: f32) -> f32 {
    let disc = r0 * r0 * (mu * mu - 1.0) + radius * radius;
    -r0 * mu + disc.max(0.0).sqrt()
}

/// Distance to the ground along the ray, if it meets it.
fn ray_hits_ground(r0: f32, mu: f32) -> Option<f32> {
    if mu >= 0.0 {
        return None;
    }
    let disc = r0 * r0 * (mu * mu - 1.0) + EARTH_R * EARTH_R;
    if disc < 0.0 {
        return None;
    }
    Some((-r0 * mu - disc.sqrt()).max(0.0))
}

fn transmittance(depth: [f32; 3], haze: f32) -> Vec3 {
    let tau = RAYLEIGH * depth[0] + Vec3::splat(MIE * MIE_EXT * haze * depth[1]) + OZONE * depth[2];
    Vec3::new((-tau.x).exp(), (-tau.y).exp(), (-tau.z).exp())
}

fn rayleigh_phase(c: f32) -> f32 {
    3.0 / (16.0 * std::f32::consts::PI) * (1.0 + c * c)
}

/// Cornette-Shanks aerosol phase function.
fn mie_phase(c: f32, g: f32) -> f32 {
    let g2 = g * g;
    3.0 / (8.0 * std::f32::consts::PI) * ((1.0 - g2) * (1.0 + c * c))
        / ((2.0 + g2) * (1.0 + g2 - 2.0 * g * c).max(1e-4).powf(1.5))
}

/// Elevation (radians) of the centre of table row `v` in 0..1.
pub fn lut_elevation(v: f32) -> f32 {
    let s = (v - 0.5) * 2.0;
    s.signum() * s * s * std::f32::consts::FRAC_PI_2
}

/// Table row coordinate (0..1) of an elevation in radians.
pub fn lut_row(el: f32) -> f32 {
    0.5 + 0.5 * el.signum() * (el.abs() / std::f32::consts::FRAC_PI_2).min(1.0).sqrt()
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Real spherical harmonics up to order 2 in direction d.
fn sh_basis(d: Vec3) -> [f32; 9] {
    [
        0.282_095,
        0.488_603 * d.y,
        0.488_603 * d.z,
        0.488_603 * d.x,
        1.092_548 * d.x * d.y,
        1.092_548 * d.y * d.z,
        0.315_392 * (3.0 * d.z * d.z - 1.0),
        1.092_548 * d.x * d.z,
        0.546_274 * (d.x * d.x - d.y * d.y),
    ]
}

/// Evaluate irradiance SH (as produced in `SkyState::sh`) for a surface normal.
pub fn sh_irradiance(sh: &[Vec3; 9], n: Vec3) -> Vec3 {
    let y = sh_basis(n);
    sh.iter()
        .zip(y)
        .map(|(c, y)| *c * y)
        .fold(Vec3::ZERO, |a, b| a + b)
}

/// The camera's white: the colour of the noon daylight on a white sheet (sun at 60° and
/// a clear sky), which the whole model is divided by - a camera set to daylight.
fn daylight_white() -> Vec3 {
    static WHITE: std::sync::OnceLock<Vec3> = std::sync::OnceLock::new();
    *WHITE.get_or_init(|| {
        let input = SkyInput {
            sun_dir: Vec3::new(0.5, 0.0, 0.866),
            ..Default::default()
        };
        let raw = SkyState::compute_raw(&input);
        let e = raw.sun * input.sun_dir.z + raw.sky_horizontal;
        e / e.dot(Vec3::new(0.2126, 0.7152, 0.0722))
    })
}

struct RawSky {
    sun: Vec3,
    lut: Vec<Vec3>,
    sky_horizontal: Vec3,
}

impl SkyState {
    /// The sky, the sun and the ambient light for this input.
    pub fn compute(input: &SkyInput) -> SkyState {
        let raw = Self::compute_raw(input);
        let white = daylight_white();
        let wb = |c: Vec3| c / white;
        let night_light = input.night_light.clamp(0.0, 4.0);
        let tint = input
            .tint
            .map(|t| t.clamp(Vec3::splat(0.25), Vec3::splat(4.0)));
        let s = input.sun_dir.normalize_or_zero();
        let sun_disc = wb(raw.sun) * tint[0];
        let sun = sun_disc * input.sun_visibility.clamp(0.0, 1.0);
        // the clear sky, greyed and evened out under a closed cover (a CIE overcast sky:
        // the zenith three times as bright as the horizon), holding about a third of what
        // sun and sky together gave, less under rain
        let clear_global =
            (raw.sun * s.z.max(0.0) + raw.sky_horizontal).dot(Vec3::new(0.2126, 0.7152, 0.0722));
        let overcast_e = clear_global * 0.34 * (1.0 - 0.45 * input.rain.clamp(0.0, 1.0));
        let overcast_zenith = overcast_e * 9.0 / (7.0 * std::f32::consts::PI);
        let grey = Vec3::new(0.96, 0.98, 1.0);
        let oc = input.overcast.clamp(0.0, 1.0);
        let (w, h) = (SKY_LUT_W as usize, SKY_LUT_H as usize);
        let mut lut: Vec<Vec3> = Vec::with_capacity(w * h);
        for row in 0..h {
            let el = lut_elevation((row as f32 + 0.5) / h as f32);
            for col in 0..w {
                let clear = wb(raw.lut[row * w + col]) * tint[1];
                let cover = grey * overcast_zenith * (1.0 + 2.0 * el.max(0.0).sin()) / 3.0;
                let l = if el >= 0.0 {
                    clear.lerp(cover, oc)
                } else {
                    clear * (1.0 - oc)
                };
                lut.push(l + NIGHT_SKY * night_light);
            }
        }
        // irradiance on a horizontal surface from the open sky
        let sun_az = s.y.atan2(s.x);
        let mut sky_horizontal = Vec3::ZERO;
        let az_step = std::f32::consts::PI / w as f32;
        for row in (h / 2)..h {
            let (e0, e1) = (
                lut_elevation(row as f32 / h as f32),
                lut_elevation((row + 1) as f32 / h as f32),
            );
            let el = lut_elevation((row as f32 + 0.5) / h as f32);
            for col in 0..w {
                sky_horizontal +=
                    lut[row * w + col] * el.sin().max(0.0) * el.cos() * az_step * (e1 - e0) * 2.0;
            }
        }
        // The SH of the upper half as a street sees it: the lowest part of the sky is hidden
        // behind houses and trees, which throw back what the sun and the sky give them -
        // warm where they face the sun, dim where they turn away from it. Taken as open sky,
        // the shade was as blue as the sky and a low sun lit it from all round.
        let ground_e = sun * s.z.max(0.0) + sky_horizontal;
        let mut sh = [Vec3::ZERO; 9];
        for row in (h / 2)..h {
            let (e0, e1) = (
                lut_elevation(row as f32 / h as f32),
                lut_elevation((row + 1) as f32 / h as f32),
            );
            let el = lut_elevation((row as f32 + 0.5) / h as f32);
            let d_el = e1 - e0;
            let hidden =
                SURROUND_MAX * (1.0 - smoothstep(0.0, SURROUND_ELEVATION.to_radians(), el));
            for col in 0..w {
                let l = lut[row * w + col];
                let az = (col as f32 + 0.5) * az_step;
                // seen towards the sun, a facade is in its own shade; away from it, sunlit
                let facing_sun = (-az.cos()).max(0.0);
                let facade_e = sun * (s.z.max(0.0).powi(2) - 1.0).abs().sqrt() * facing_sun * 0.8
                    + sky_horizontal * 0.5
                    + ground_e * input.ground_albedo * 0.5;
                let facade = facade_e * (SURROUND_ALBEDO / std::f32::consts::PI) * tint[2];
                let l = l.lerp(facade, hidden);
                for side in [-1.0f32, 1.0] {
                    let a = sun_az + side * az;
                    let d = Vec3::new(el.cos() * a.cos(), el.cos() * a.sin(), el.sin());
                    let dw = el.cos() * az_step * d_el;
                    for (k, y) in sh_basis(d).iter().enumerate() {
                        sh[k] += l * *y * dw;
                    }
                }
            }
        }
        // the ground: lit by the sun and the sky, seen as the lower half of the sphere
        let ground = (sun * s.z.max(0.0) + sky_horizontal) * input.ground_albedo
            / std::f32::consts::PI
            * tint[2];
        let lower = ground_sh(ground);
        for k in 0..9 {
            sh[k] += lower[k];
        }
        // the table's own lower half: the distant ground seen through the air
        for row in 0..h / 2 {
            for col in 0..w {
                let i = row * w + col;
                let el = lut_elevation((row as f32 + 0.5) / h as f32);
                let fade = (-el).sin().clamp(0.0, 1.0).powf(0.35);
                lut[i] = lut[i].lerp(ground, fade * 0.85 + 0.15 * oc);
            }
        }
        // cosine lobe convolution, and the eye's own white balance in the shade: it takes
        // the sky's blue in part for white (a linear map, so it applies to every coefficient)
        let band = [
            std::f32::consts::PI,
            2.0 * std::f32::consts::PI / 3.0,
            std::f32::consts::PI / 4.0,
        ];
        let lum = Vec3::new(0.2126, 0.7152, 0.0722);
        for (k, c) in sh.iter_mut().enumerate() {
            *c *= band[if k == 0 {
                0
            } else if k < 4 {
                1
            } else {
                2
            }];
            let grey = Vec3::splat(c.dot(lum));
            *c = grey + (*c - grey) * (1.0 - SHADE_ADAPTATION);
        }
        let lut_scale = lut.iter().map(|l| l.max_element()).fold(1e-6f32, f32::max);
        let lut_out = lut
            .iter()
            .map(|l| [l.x / lut_scale, l.y / lut_scale, l.z / lut_scale, 1.0])
            .collect();
        // the light the eye adapts to: a low sun still falls fully on the walls that face it,
        // which a horizontal surface alone does not tell (a sunlit facade at seven in the
        // evening came out washed out)
        let sun_facing =
            s.z.max(0.0) + (1.0 - s.z.max(0.0)) * LOW_SUN_WALLS * (s.z * 20.0).clamp(0.0, 1.0);
        // (the exposure is that of the night as it is by default: a darker night stays dark)
        let night_lost = (1.0 - night_light) * std::f32::consts::PI * NIGHT_SKY.dot(lum);
        let e_ref =
            ((sun * sun_facing + sky_horizontal).dot(lum) + night_lost).max(0.0) + ARTIFICIAL;
        SkyState {
            input: *input,
            sun,
            sun_disc,
            lut: lut_out,
            lut_scale,
            sh,
            sky_horizontal,
            ground,
            exposure: exposure_for(e_ref),
        }
    }

    /// Single scattering (plus the multiple-scattering estimate) before white balance,
    /// clouds and tints.
    fn compute_raw(input: &SkyInput) -> RawSky {
        let table = depth_table();
        let s = input.sun_dir.normalize_or_zero();
        let haze = input.haze.max(0.0);
        let r0 = EARTH_R + OBSERVER_H;
        let sun = SUN_E0 * transmittance(table.lookup(OBSERVER_H, s.z), haze);
        let (w, h) = (SKY_LUT_W as usize, SKY_LUT_H as usize);
        let mie = MIE * haze;
        let mut lut = Vec::with_capacity(w * h);
        let horiz = Vec3::new(s.x, s.y, 0.0).length();
        let sun_el_cos = horiz;
        for row in 0..h {
            let el = lut_elevation((row as f32 + 0.5) / h as f32);
            let mu = el.sin();
            let t_max = ray_hits_ground(r0, mu).unwrap_or_else(|| ray_exit(r0, mu, ATMO_R));
            for col in 0..w {
                let az = (col as f32 + 0.5) / w as f32 * std::f32::consts::PI;
                // the sun lies at azimuth 0 in this frame
                let d = Vec3::new(el.cos() * az.cos(), el.cos() * az.sin(), mu);
                let sl = Vec3::new(sun_el_cos, 0.0, s.z);
                let c = d.dot(sl);
                let (pr, pm) = (rayleigh_phase(c), mie_phase(c, MIE_G));
                let n = 20;
                let mut acc = Vec3::ZERO;
                let mut view_depth = [0.0f32; 3];
                let mut prev_t = 0.0f32;
                for k in 0..n {
                    let b = (k + 1) as f32 / n as f32;
                    let t1 = b * b * t_max;
                    let t = 0.5 * (prev_t + t1);
                    let dt = t1 - prev_t;
                    prev_t = t1;
                    let hh = altitude(r0, mu, t);
                    let dens = densities(hh);
                    // the view path up to the middle of this step
                    let half = [
                        view_depth[0] + dens[0] * dt * 0.5,
                        view_depth[1] + dens[1] * dt * 0.5,
                        view_depth[2] + dens[2] * dt * 0.5,
                    ];
                    view_depth[0] += dens[0] * dt;
                    view_depth[1] += dens[1] * dt;
                    view_depth[2] += dens[2] * dt;
                    let r = (hh + EARTH_R).max(1.0);
                    let mu_s = ((r0 * s.z + t * c) / r).clamp(-1.0, 1.0);
                    let t_view = transmittance(half, haze);
                    let t_sun = transmittance(table.lookup(hh, mu_s), haze);
                    let scat_r = RAYLEIGH * dens[0];
                    let scat_m = mie * dens[1];
                    let single = (scat_r * pr + Vec3::splat(scat_m * pm)) * t_sun;
                    // the higher orders: light scattered more than once arrives from all
                    // around, and still some while the sun is just below the horizon
                    let lit = ((mu_s + 0.2) / 1.2).clamp(0.0, 1.0);
                    let t_up = transmittance(table.lookup(hh, 1.0), haze);
                    // (aerosols scatter forward: little of their light goes round again)
                    let multi = (scat_r + Vec3::splat(scat_m * MULTI_MIE))
                        * (t_sun * 0.6 + t_up * 0.4 * lit * lit)
                        * (MULTI / (4.0 * std::f32::consts::PI));
                    acc += t_view * (single + multi) * dt;
                }
                lut.push(acc * SUN_E0);
            }
        }
        // irradiance of a horizontal surface from the clear sky
        let mut sky_horizontal = Vec3::ZERO;
        let az_step = std::f32::consts::PI / w as f32;
        for row in (h / 2)..h {
            let (e0, e1) = (
                lut_elevation(row as f32 / h as f32),
                lut_elevation((row + 1) as f32 / h as f32),
            );
            let el = lut_elevation((row as f32 + 0.5) / h as f32);
            for col in 0..w {
                sky_horizontal +=
                    lut[row * w + col] * el.sin().max(0.0) * el.cos() * az_step * (e1 - e0) * 2.0;
            }
        }
        RawSky {
            sun,
            lut,
            sky_horizontal,
        }
    }
}

/// The SH of a uniformly bright lower hemisphere (the ground).
fn ground_sh(l: Vec3) -> [Vec3; 9] {
    // projection of the lower half-sphere indicator: Y00 · 2π, Y10 · -π, Y20 · 0
    let mut out = [Vec3::ZERO; 9];
    out[0] = l * (0.282_095 * 2.0 * std::f32::consts::PI);
    out[2] = l * (-0.488_603 * std::f32::consts::PI);
    out
}

/// Pre-exposure for a reference irradiance: full exposure by day, only part of the way
/// up at night (a street at night still looks dark - the eye does not adapt completely).
pub fn exposure_for(e_ref: f32) -> f32 {
    const ADAPT: f32 = 0.8;
    // a quarter of a stop over "mid grey in full light = 0.18": the tone curve's shoulder
    // holds the sunlit white, and the shade does not sink
    1.25 * std::f32::consts::PI / (e_ref.max(1e-6).powf(ADAPT) * DAY_REFERENCE.powf(1.0 - ADAPT))
}

/// A half float from a float (for the sky table's upload).
pub fn f16_bits(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mant = bits & 0x007f_ffff;
    if v.is_nan() {
        return 0x7e00;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    if exp <= 0 {
        if exp < -10 {
            return sign;
        }
        let m = (mant | 0x0080_0000) >> (1 - exp);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    sign | ((exp as u16) << 10) | (((mant + 0x1000) >> 13) as u16).min(0x3ff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: Vec3) -> f32 {
        c.dot(Vec3::new(0.2126, 0.7152, 0.0722))
    }

    fn sun_at(deg: f32) -> Vec3 {
        let a = deg.to_radians();
        Vec3::new(0.0, -a.cos(), a.sin())
    }

    #[test]
    fn noon_light_is_white_and_the_shade_realistic() {
        let s = SkyState::compute(&SkyInput {
            sun_dir: sun_at(60.0),
            ..Default::default()
        });
        let global = s.sun * 0.866 + s.sky_horizontal;
        // the camera is balanced for this light
        assert!(
            (global.x / global.y - 1.0).abs() < 0.02 && (global.z / global.y - 1.0).abs() < 0.02,
            "{global:?}"
        );
        // about 11 units in all; the sky gives 12..25 % of it
        assert!((lum(global) - 11.0).abs() < 2.5, "{}", lum(global));
        let share = lum(s.sky_horizontal) / lum(global);
        assert!((0.12..0.25).contains(&share), "sky share {share}");
        // the sun is a little warmer than the sky, the sky bluer than the sun
        assert!(s.sun.x > s.sun.z && s.sky_horizontal.z > s.sky_horizontal.x);
        // SH: a surface facing up gets the sky plus a little from nowhere below
        let up = sh_irradiance(&s.sh, Vec3::Z);
        assert!(
            (lum(up) / lum(s.sky_horizontal) - 1.0).abs() < 0.1,
            "{up:?} vs {:?}",
            s.sky_horizontal
        );
        // a wall gets half the ground's bounce and some of the sky and the houses across the
        // street, the ground's underside only the bounce
        let wall = sh_irradiance(&s.sh, Vec3::X);
        let down = sh_irradiance(&s.sh, -Vec3::Z);
        assert!(
            lum(wall) > 0.6 * lum(down) && lum(down) > 0.0,
            "wall {wall:?} down {down:?}"
        );
        // the shade is bluish, but not as blue as the sky
        let shade_blue = up.z / up.x;
        assert!(
            shade_blue > 1.1 && shade_blue < s.sky_horizontal.z / s.sky_horizontal.x,
            "{up:?}"
        );
    }

    #[test]
    fn low_sun_is_golden_and_twilight_blue() {
        let low = SkyState::compute(&SkyInput {
            sun_dir: sun_at(9.6),
            ..Default::default()
        });
        assert!(
            low.sun.x > low.sun.y * 1.15 && low.sun.y > low.sun.z * 1.2,
            "{:?}",
            low.sun
        );
        let dusk = SkyState::compute(&SkyInput {
            sun_dir: sun_at(-4.0),
            ..Default::default()
        });
        assert!(lum(dusk.sun) < 1e-3, "{:?}", dusk.sun);
        assert!(
            dusk.sky_horizontal.z > dusk.sky_horizontal.x,
            "{:?}",
            dusk.sky_horizontal
        );
        let night = SkyState::compute(&SkyInput {
            sun_dir: sun_at(-30.0),
            ..Default::default()
        });
        assert!(
            lum(night.sky_horizontal) < 1e-3 && lum(night.sky_horizontal) > 1e-5,
            "{:?}",
            night.sky_horizontal
        );
        // exposure rises into the night, but not all the way
        assert!(night.exposure > low.exposure * 100.0);
        // a white wall under the street lamps comes out clearly below white, one lit by
        // the night sky alone dark but not black
        let lamp = 0.9 / std::f32::consts::PI * ARTIFICIAL * night.exposure;
        let sky = 0.3 / std::f32::consts::PI * lum(night.sky_horizontal) * night.exposure;
        assert!(
            (0.1..0.5).contains(&lamp) && (0.003..0.03).contains(&sky),
            "lamp {lamp} sky {sky}"
        );
    }

    #[test]
    fn overcast_is_grey_and_darker() {
        let clear = SkyState::compute(&SkyInput {
            sun_dir: sun_at(40.0),
            ..Default::default()
        });
        let grey = SkyState::compute(&SkyInput {
            sun_dir: sun_at(40.0),
            sun_visibility: 0.0,
            overcast: 1.0,
            ..Default::default()
        });
        let g = grey.sky_horizontal;
        assert!((g.z / g.x) < 1.15, "{g:?}");
        let clear_global = lum(clear.sun * sun_at(40.0).z + clear.sky_horizontal);
        assert!(
            (0.2..0.5).contains(&(lum(g) / clear_global)),
            "{} of {}",
            lum(g),
            clear_global
        );
        assert!(grey.exposure > clear.exposure * 1.5);
    }

    /// `cargo test -p omsi-render sky_report -- --nocapture --ignored`: the sky as the
    /// screen shows it (sRGB after the exposure, before the metering) for tuning.
    #[test]
    #[ignore]
    fn sky_report() {
        let srgb = |c: f32| {
            let c = c.clamp(0.0, 1.0);
            (if c <= 0.003_130_8 {
                c * 12.92
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            }) * 255.0
        };
        for (sun_el, oc) in [
            (60.0f32, 0.0f32),
            (22.0, 0.0),
            (8.0, 0.0),
            (1.0, 0.0),
            (-4.0, 0.0),
            (-20.0, 0.0),
            (40.0, 1.0),
        ] {
            let input = SkyInput {
                sun_dir: sun_at(sun_el),
                overcast: oc,
                sun_visibility: 1.0 - oc,
                ..Default::default()
            };
            let st = SkyState::compute(&input);
            let e = st.exposure;
            println!(
                "sun {sun_el:5.1}° overcast {oc}: sun {:?} sky_h {:?} exposure {e:.3} (log2 {:.2})",
                st.sun,
                st.sky_horizontal,
                e.log2()
            );
            let sun_az = input.sun_dir.y.atan2(input.sun_dir.x);
            for rel_az in [0.0f32, 90.0, 180.0] {
                let mut line = format!("  az {rel_az:5.0}:");
                for el in [2.0f32, 8.0, 20.0, 45.0, 89.0] {
                    let a = sun_az + rel_az.to_radians();
                    let d = Vec3::new(
                        el.to_radians().cos() * a.cos(),
                        el.to_radians().cos() * a.sin(),
                        el.to_radians().sin(),
                    );
                    // the table cell nearest to d
                    let az = {
                        let (s2, d2) = (
                            glam::Vec2::new(input.sun_dir.x, input.sun_dir.y).normalize(),
                            glam::Vec2::new(d.x, d.y).normalize(),
                        );
                        s2.dot(d2).clamp(-1.0, 1.0).acos()
                    };
                    let col = ((az / std::f32::consts::PI) * SKY_LUT_W as f32)
                        .floor()
                        .min(SKY_LUT_W as f32 - 1.0) as usize;
                    let row = (lut_row(el.to_radians()) * SKY_LUT_H as f32)
                        .floor()
                        .min(SKY_LUT_H as f32 - 1.0) as usize;
                    let l = st.lut[row * SKY_LUT_W as usize + col];
                    let c = Vec3::new(l[0], l[1], l[2]) * st.lut_scale * e;
                    line += &format!(
                        "  {el:2.0}°({:3.0},{:3.0},{:3.0})",
                        srgb(c.x),
                        srgb(c.y),
                        srgb(c.z)
                    );
                }
                println!("{line}");
            }
            let n = |v: Vec3| format!("({:3.0},{:3.0},{:3.0})", srgb(v.x), srgb(v.y), srgb(v.z));
            // a grey card (albedo 0.18) facing up, facing the sun's side, facing away
            let up = (st.sun * input.sun_dir.z.max(0.0) + sh_irradiance(&st.sh, Vec3::Z)) * 0.18
                / std::f32::consts::PI
                * e;
            let shade = sh_irradiance(&st.sh, Vec3::Z) * 0.18 / std::f32::consts::PI * e;
            let wall_away = sh_irradiance(
                &st.sh,
                -Vec3::new(input.sun_dir.x, input.sun_dir.y, 0.0).normalize(),
            ) * 0.18
                / std::f32::consts::PI
                * e;
            println!(
                "  grey card: sunlit up {}  shaded up {}  wall facing away {}",
                n(up),
                n(shade),
                n(wall_away)
            );
        }
    }

    #[test]
    fn half_floats() {
        for v in [0.0f32, 1.0, -2.5, 0.5, 65504.0, 1e-3, 3.140625] {
            let b = f16_bits(v);
            let sign = if b & 0x8000 != 0 { -1.0 } else { 1.0 };
            let e = ((b >> 10) & 0x1f) as i32;
            let m = (b & 0x3ff) as f32;
            let back = if e == 0 {
                sign * m / 1024.0 * 2f32.powi(-14)
            } else {
                sign * (1.0 + m / 1024.0) * 2f32.powi(e - 15)
            };
            assert!((back - v).abs() <= v.abs() * 1e-3 + 1e-6, "{v} -> {back}");
        }
        assert_eq!(f16_bits(1e9), 0x7c00);
    }
}
