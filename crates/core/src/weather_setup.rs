//! The weather of a session: its `.owt` file, the sky and clouds it makes, and how wet the streets are.

use super::*;

pub(crate) const CUSTOM_CLOUDS: [&str; 5] = [
    "No clouds",
    "Cumulus 1",
    "Cumulus 2",
    "Cumulus 3",
    "Overcast 1",
];
pub(crate) const CUSTOM_PRECIP: [&str; 3] = ["None", "Rain", "Snow"];

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CustomWeather {
    pub visibility_m: f32,
    pub brightness: f32,
    pub wind_dir: f32,
    pub wind_speed: f32,
    pub temp_c: f32,
    pub humidity: f32,
    pub pressure: f32,
    pub cloud: usize,
    pub cloud_base_m: f32,
    pub precip: i32,
    pub precip_intensity: f32,
    pub road_wetness: f32,
    pub snow_cover: bool,
    pub snow_on_road: bool,
}
impl Default for CustomWeather {
    fn default() -> Self {
        Self {
            visibility_m: 50_000.0,
            brightness: 1.0,
            wind_dir: 0.0,
            wind_speed: 0.0,
            temp_c: 15.0,
            humidity: 51.0,
            pressure: 1013.0,
            cloud: 0,
            cloud_base_m: 50.0,
            precip: 0,
            precip_intensity: 32.0,
            road_wetness: 0.0,
            snow_cover: false,
            snow_on_road: false,
        }
    }
}
impl CustomWeather {
    pub(crate) fn normalize(&mut self) {
        self.visibility_m = self.visibility_m.clamp(50.0, 50_000.0);
        self.brightness = self.brightness.clamp(0.0, 1.5);
        self.wind_dir = self.wind_dir.rem_euclid(360.0);
        self.wind_speed = self.wind_speed.clamp(0.0, 50.0);
        self.temp_c = self.temp_c.clamp(-40.0, 50.0);
        self.humidity = self.humidity.clamp(0.0, 100.0);
        self.pressure = self.pressure.clamp(900.0, 1100.0);
        self.cloud = self.cloud.min(CUSTOM_CLOUDS.len() - 1);
        self.cloud_base_m = self.cloud_base_m.clamp(50.0, 5000.0);
        self.precip = self.precip.clamp(0, 2);
        self.precip_intensity = self.precip_intensity.clamp(0.0, 255.0);
        self.road_wetness = self.road_wetness.clamp(0.0, 1.0);
    }
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let body = text
            .strip_prefix("custom:")
            .or_else(|| text.strip_prefix("CUSTOM:"))?;
        let mut c = Self::default();
        for part in body.split(';') {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            let n = value.trim().parse::<f32>().ok();
            match key.trim().to_ascii_lowercase().as_str() {
                "vis" => {
                    if let Some(v) = n {
                        c.visibility_m = v
                    }
                }
                "br" => {
                    if let Some(v) = n {
                        c.brightness = v
                    }
                }
                "wd" => {
                    if let Some(v) = n {
                        c.wind_dir = v
                    }
                }
                "ws" => {
                    if let Some(v) = n {
                        c.wind_speed = v
                    }
                }
                "t" => {
                    if let Some(v) = n {
                        c.temp_c = v
                    }
                }
                "rh" => {
                    if let Some(v) = n {
                        c.humidity = v
                    }
                }
                "p" => {
                    if let Some(v) = n {
                        c.pressure = v
                    }
                }
                "c" => {
                    if let Some(v) = n {
                        c.cloud = v.round().max(0.0) as usize
                    }
                }
                "cb" => {
                    if let Some(v) = n {
                        c.cloud_base_m = v
                    }
                }
                "pt" => {
                    if let Some(v) = n {
                        c.precip = v.round() as i32
                    }
                }
                "pi" => {
                    if let Some(v) = n {
                        c.precip_intensity = v
                    }
                }
                "wet" => {
                    if let Some(v) = n {
                        c.road_wetness = v
                    }
                }
                "snow" => {
                    if let Some(v) = n {
                        c.snow_cover = v >= 0.5
                    }
                }
                "snowroad" => {
                    if let Some(v) = n {
                        c.snow_on_road = v >= 0.5
                    }
                }
                _ => {}
            }
        }
        c.normalize();
        Some(c)
    }
    pub(crate) fn encode(&self) -> String {
        let mut c = self.clone();
        c.normalize();
        format!(
            "custom:vis={:.0};br={:.2};wd={:.0};ws={:.1};t={:.1};rh={:.0};p={:.0};c={};cb={:.0};pt={};pi={:.0};wet={:.2};snow={};snowroad={}",
            c.visibility_m,
            c.brightness,
            c.wind_dir,
            c.wind_speed,
            c.temp_c,
            c.humidity,
            c.pressure,
            c.cloud,
            c.cloud_base_m,
            c.precip,
            c.precip_intensity,
            c.road_wetness,
            c.snow_cover as u8,
            c.snow_on_road as u8
        )
    }
    pub(crate) fn from_weather(
        w: &::content::weather::Weather,
        brightness: f32,
        wetness: f32,
    ) -> Self {
        let kind = w.clouds.0.trim().to_ascii_lowercase();
        let cloud = if kind.starts_with("cumulus 1") {
            1
        } else if kind.starts_with("cumulus 2") {
            2
        } else if kind.starts_with("cumulus 3") {
            3
        } else if kind.starts_with("overcast") {
            4
        } else {
            0
        };
        let mut c = Self {
            visibility_m: w.fog.0,
            brightness,
            wind_dir: w.wind.0,
            wind_speed: w.wind.1,
            temp_c: w.temp.0,
            humidity: relative_humidity(w.temp.0, w.temp.1),
            pressure: if w.pressure > 0.0 { w.pressure } else { 1013.0 },
            cloud,
            cloud_base_m: w.clouds.1.max(50.0),
            precip: w.precip.first().copied().unwrap_or(0.0).round() as i32,
            precip_intensity: w.precip.get(1).copied().unwrap_or(32.0),
            road_wetness: wetness,
            snow_cover: w.snow,
            snow_on_road: w.snow_on_road,
        };
        c.normalize();
        c
    }
    pub(crate) fn to_weather(&self) -> ::content::weather::Weather {
        let mut c = self.clone();
        c.normalize();
        let cloud = match c.cloud {
            1 => "Cumulus 1",
            2 => "Cumulus 2",
            3 => "Cumulus 3",
            4 => "Overcast 1",
            _ => "-1",
        };
        ::content::weather::Weather {
            path: PathBuf::from(c.encode()),
            name: "Custom weather".into(),
            description: "User-defined weather".into(),
            fog: (c.visibility_m, 1.0),
            wind: (c.wind_dir, c.wind_speed),
            temp: (c.temp_c, absolute_humidity(c.temp_c, c.humidity)),
            pressure: c.pressure,
            clouds: (cloud.into(), c.cloud_base_m),
            precip: vec![c.precip as f32, c.precip_intensity, 0.0, 0.0, 0.0],
            ground_wet: [c.road_wetness * 255.0, 0.0, 0.0],
            snow: c.snow_cover,
            snow_on_road: c.snow_on_road,
        }
    }
}
fn saturation_vapour_pressure(temp_c: f32) -> f32 {
    6.112 * ((17.67 * temp_c) / (temp_c + 243.5)).exp()
}
pub(crate) fn absolute_humidity(temp_c: f32, relative: f32) -> f32 {
    let vapour = saturation_vapour_pressure(temp_c) * relative.clamp(0.0, 100.0) / 100.0;
    (216.7 * vapour / (temp_c + 273.15).max(1.0)).max(0.0)
}
pub(crate) fn relative_humidity(temp_c: f32, absolute: f32) -> f32 {
    let vapour = absolute.max(0.0) * (temp_c + 273.15).max(1.0) / 216.7;
    (vapour / saturation_vapour_pressure(temp_c).max(0.001) * 100.0).clamp(0.0, 100.0)
}
pub(crate) fn dew_point_c(temp_c: f32, relative: f32) -> f32 {
    let rh = (relative.clamp(0.1, 100.0) / 100.0).ln();
    let g = rh + 17.67 * temp_c / (243.5 + temp_c);
    243.5 * g / (17.67 - g)
}
pub(crate) fn custom_weather(text: Option<&str>) -> Option<CustomWeather> {
    text.and_then(CustomWeather::parse)
}

/// Weather from `--weather`, else the clear-sky default.
pub(crate) fn load_weather(args: &Args) -> ::content::weather::Weather {
    let rel = args
        .weather
        .clone()
        .filter(|w| !weather_cycle::is_cycle(Some(w)))
        .unwrap_or_else(|| "Weather/#CAVOK.owt".into());
    if let Some(w) = CustomWeather::parse(&rel).map(|c| c.to_weather()) {
        log::info!(
            "weather custom: fog range {} m, precip {:?}, temp {:?}",
            w.fog.0,
            w.precip,
            w.temp
        );
        scene::SNOW_WEATHER.store(w.snow, std::sync::atomic::Ordering::Relaxed);
        ::simulation::host::set_ambient_weather(w.temp.0, w.temp.1);
        return w;
    }
    // OMSI 2's current weather: `metar:<ICAO>` fetches the airport's report
    let loaded = if rel.starts_with(REPORT) {
        // a report the host or server tells (see `report_wire`): its values, no download
        Ok(from_report(&rel).unwrap_or_else(|| ::content::weather::from_metar("", "CAVOK")))
    } else {
        match rel
            .strip_prefix("metar:")
            .or_else(|| rel.strip_prefix("METAR:"))
        {
            Some(icao) => Ok(fetch_metar(icao.trim())),
            None => ::content::weather::Weather::load(&::mods::resolve_path(&args.root, &rel)),
        }
    };
    match loaded {
        Ok(w) => {
            log::info!(
                "weather {}: fog range {} m, precip {:?}, temp {:?}",
                w.name,
                w.fog.0,
                w.precip,
                w.temp
            );
            let mut w = w;
            // No weather chosen: the clear default (#CAVOK, no cloud at all) gets a few fair-
            // weather cumulus clouds, as long as clouds are wanted - a sky without a single
            // cloud was the first thing that looked wrong.
            if args.weather.is_none()
                && CLOUDS.load(std::sync::atomic::Ordering::Relaxed)
                && w.clouds.0.trim().starts_with("-1")
            {
                w.clouds = ("Cumulus 1".into(), 100.0);
            }
            // the vehicles ask for it while they go onto the GPU (the snow on the panes)
            scene::SNOW_WEATHER.store(w.snow, std::sync::atomic::Ordering::Relaxed);
            // and their {init} reads the temperature
            ::simulation::host::set_ambient_weather(w.temp.0, w.temp.1);
            w
        }
        Err(e) => {
            log::warn!("weather {rel}: {e}");
            scene::SNOW_WEATHER.store(false, std::sync::atomic::Ordering::Relaxed);
            ::content::weather::Weather {
                fog: (50000.0, 1.0),
                ..Default::default()
            }
        }
    }
}

/// Sky gradient textures from envir.cfg uploaded into the scene.
pub(crate) fn setup_sky(
    args: &Args,
    renderer: &Renderer,
    scene: &mut Scene,
    envir: Option<&::content::Envir>,
    weather: Option<&::content::weather::Weather>,
) {
    const STOCK: [&str; 3] = [
        "Texture\\himmel01.bmp",
        "Texture\\himmel04.bmp",
        "Texture\\himmel05.bmp",
    ];
    let names = envir
        .map(|e| e.sky_textures.clone())
        .unwrap_or_else(|| STOCK.map(String::from));
    let mut ids = Vec::new();
    for (n, stock) in names.iter().zip(STOCK) {
        let p = ::mods::resolve_path(&args.root, n);
        // a sky pack's picture that cannot be read leaves the stock one in its place: giving
        // up here took the clouds with it, whatever the weather (#749)
        let img = ::texture::decode_file(&p).or_else(|e| {
            log::warn!("sky texture {}: {e}", p.display());
            ::texture::decode_file(&::mods::resolve_path(&args.root, stock))
        });
        match img {
            Ok(img) => ids.push(renderer.add_texture(scene, &img, false)),
            Err(e) => {
                log::warn!("sky texture {stock}: {e}");
                return;
            }
        }
    }
    // the weather's cloud type (Weather/clouds.cfg: Cumulus 1..3, Overcast 1) with its own
    // texture, whose alpha is the clouds' shape; `Texture\clouds.tga` when there is none
    let kind = weather
        .map(|w| w.clouds.0.trim().to_string())
        .unwrap_or_default();
    let typed = cloud_texture(&args.root, &kind);
    let cover = typed.or_else(|| {
        ::texture::decode_file(&::mods::resolve_path(&args.root, "Texture\\clouds.tga")).ok()
    });
    let t = Instant::now();
    let field = cloud_field(cover.as_ref());
    log::debug!(
        "cloud field made in {:.0} ms",
        t.elapsed().as_secs_f64() * 1000.0
    );
    let clouds = Some(renderer.add_texture(scene, &field, true));
    renderer.set_sky_textures_clouds(scene, [ids[0], ids[1], ids[2]], clouds);
}

/// Edge of the cloud field texture (texels); it tiles.
const FIELD: usize = 512;

/// The texture both sky shaders draw their clouds from, seamless in both directions:
/// R the weather's own cloud picture (Weather/clouds.cfg) - where its clouds are, the sky
/// has more; G the shape of the cumulus, fractal noise bent by more noise, equalised so
/// that a threshold of 1 - f covers exactly the fraction f of the sky; B billows for the
/// cauliflower edges; A how tall each cloud grows. Stored in sRGB bytes so that the
/// shader reads the values back as they were made.
pub(crate) fn cloud_field(cover: Option<&::texture::Image>) -> ::texture::Image {
    let n = FIELD;
    let lattice = |x: i64, y: i64, period: i64, seed: u32| -> f32 {
        let (x, y) = (x.rem_euclid(period) as u32, y.rem_euclid(period) as u32);
        let mut h = x.wrapping_mul(0x8da6_b343)
            ^ y.wrapping_mul(0xd816_3841)
            ^ seed.wrapping_mul(0xcb1a_b31f);
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1_e995);
        h ^= h >> 15;
        (h & 0xffff) as f32 / 65535.0
    };
    // value noise with `period` cells across the tile, at a point of the tile (0..1)
    let noise = |u: f32, v: f32, period: i64, seed: u32| -> f32 {
        let (x, y) = (u * period as f32, v * period as f32);
        let (ix, iy) = (x.floor() as i64, y.floor() as i64);
        let (fx, fy) = (x - ix as f32, y - iy as f32);
        let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let a = lattice(ix, iy, period, seed)
            + (lattice(ix + 1, iy, period, seed) - lattice(ix, iy, period, seed)) * sx;
        let b = lattice(ix, iy + 1, period, seed)
            + (lattice(ix + 1, iy + 1, period, seed) - lattice(ix, iy + 1, period, seed)) * sx;
        a + (b - a) * sy
    };
    let fbm = |u: f32, v: f32, period: i64, octaves: u32, seed: u32| -> f32 {
        let (mut sum, mut amp, mut norm, mut p) = (0.0, 0.5, 0.0, period);
        for o in 0..octaves {
            sum += amp * noise(u, v, p, seed + o * 101);
            norm += amp;
            amp *= 0.5;
            p *= 2;
        }
        sum / norm
    };
    let mut shape = vec![0f32; n * n];
    let mut billow = vec![0f32; n * n];
    let mut tall = vec![0f32; n * n];
    // (rows in parallel: 512 x 512 texels of a few octaves each)
    use rayon::prelude::*;
    shape
        .par_chunks_mut(n)
        .zip(billow.par_chunks_mut(n))
        .zip(tall.par_chunks_mut(n))
        .enumerate()
        .for_each(|(y, ((s_row, b_row), t_row))| {
            let v = y as f32 / n as f32;
            for x in 0..n {
                let u = x as f32 / n as f32;
                // bend the cumulus field with a coarser one, so that clouds are not blobs on a grid
                let wu = fbm(u, v, 4, 3, 11) - 0.5;
                let wv = fbm(u, v, 4, 3, 23) - 0.5;
                s_row[x] = fbm(u + wu * 0.12, v + wv * 0.12, 6, 5, 37);
                let mut b = 0.0;
                let mut amp = 0.5;
                let mut p = 24;
                for o in 0..3 {
                    b += amp * (1.0 - (noise(u, v, p, 53 + o) * 2.0 - 1.0).abs());
                    amp *= 0.5;
                    p *= 2;
                }
                b_row[x] = b / 0.875;
                t_row[x] = fbm(u, v, 3, 2, 71);
            }
        });
    // equalise the shape: its rank, so that a threshold is a fraction of the sky
    let mut order: Vec<u32> = (0..(n * n) as u32).collect();
    order.par_sort_unstable_by(|a, b| shape[*a as usize].total_cmp(&shape[*b as usize]));
    let mut equal = vec![0f32; n * n];
    for (rank, &i) in order.iter().enumerate() {
        equal[i as usize] = rank as f32 / (n * n - 1) as f32;
    }
    let stretch = |v: &mut Vec<f32>| {
        let (lo, hi) = v
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        for x in v.iter_mut() {
            *x = (*x - lo) / (hi - lo).max(1e-6);
        }
    };
    stretch(&mut billow);
    stretch(&mut tall);
    let to_srgb = |x: f32| -> u8 {
        let x = x.clamp(0.0, 1.0);
        let s = if x <= 0.003_130_8 {
            x * 12.92
        } else {
            1.055 * x.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0 + 0.5) as u8
    };
    // the weather's picture, sampled bilinearly (it tiles as well), its brightness the cover
    let weather = |u: f32, v: f32| -> f32 {
        let Some(img) = cover else { return 0.5 };
        let (w, h) = (img.width as i64, img.height as i64);
        let (x, y) = (u * w as f32 - 0.5, v * h as f32 - 0.5);
        let (ix, iy) = (x.floor() as i64, y.floor() as i64);
        let (fx, fy) = (x - ix as f32, y - iy as f32);
        let px = |xx: i64, yy: i64| {
            let i = ((yy.rem_euclid(h) * w + xx.rem_euclid(w)) * 4) as usize;
            (img.rgba[i] as f32 + img.rgba[i + 1] as f32 + img.rgba[i + 2] as f32) / (3.0 * 255.0)
        };
        let a = px(ix, iy) + (px(ix + 1, iy) - px(ix, iy)) * fx;
        let b = px(ix, iy + 1) + (px(ix + 1, iy + 1) - px(ix, iy + 1)) * fx;
        a + (b - a) * fy
    };
    let mut rgba = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let i = y * n + x;
            let o = i * 4;
            rgba[o] = to_srgb(weather(x as f32 / n as f32, y as f32 / n as f32));
            rgba[o + 1] = to_srgb(equal[i]);
            rgba[o + 2] = to_srgb(billow[i]);
            // (alpha is not sRGB-coded)
            rgba[o + 3] = (tall[i].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    }
    ::texture::Image {
        width: n as u32,
        height: n as u32,
        rgba,
        has_alpha: true,
    }
}

/// The texture of a cloud type named in `Weather/clouds.cfg` (`[cloudtype]` name, texture,
/// size in metres, sct/ovc), as the sky shaders read it: the cover in the colour channels.
/// A scattered type's texture is white with the clouds in its alpha; an overcast one is a
/// picture of the cloud deck, its brightness is the cover.
fn cloud_texture(root: &Path, kind: &str) -> Option<::texture::Image> {
    if kind.is_empty() || kind.starts_with("-1") {
        return None;
    }
    let cfg = ::legacy_config::vfs::read(&root.join("Weather").join("clouds.cfg")).ok()?;
    let text = ::legacy_config::codepage::decode(&cfg);
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).collect();
    let mut file = None;
    for i in 0..lines.len() {
        if lines[i].eq_ignore_ascii_case("[cloudtype]")
            && lines
                .get(i + 1)
                .map(|n| n.eq_ignore_ascii_case(kind))
                .unwrap_or(false)
        {
            file = lines.get(i + 2).map(|s| s.to_string());
            break;
        }
    }
    let file = file?;
    let mut img = ::texture::decode_file(&::mods::resolve_path(
        &::mods::resolve_path(&root, "Texture"),
        &file,
    ))
    .ok()?;
    if img.has_alpha {
        for px in img.rgba.chunks_mut(4) {
            let a = px[3];
            px[0] = a;
            px[1] = a;
            px[2] = a;
            px[3] = 255;
        }
        img.has_alpha = false;
    }
    log::info!("clouds: {kind} ({file})");
    Some(img)
}

fn rel_humidity(w: &::content::weather::Weather) -> f32 {
    let t = w.temp.0.clamp(-40.0, 50.0);
    let es = 6.112 * (17.62 * t / (243.12 + t)).exp();
    let sat = 216.7 * es / (t + 273.15);
    if sat > 0.0 {
        (w.temp.1 / sat).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

struct CloudModel {
    density: f32,
    layers: [[f32; 4]; 3],
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let k = ((x - a) / (b - a)).clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

fn hash01(mut h: u32) -> f32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65535.0
}

/// Smooth noise over time `x` (one lattice step each unit), 0..1.
fn time_noise(x: f64, seed: u32) -> f32 {
    let i = x.floor();
    let f = (x - i) as f32;
    let k = f * f * (3.0 - 2.0 * f);
    let h = |n: i64| hash01((n as u32).wrapping_mul(0x9e37_79b9) ^ seed);
    let (a, b) = (h(i as i64), h(i as i64 + 1));
    a + (b - a) * k
}

/// The sky this weather makes at this date and hour. Nothing in it is fixed: humidity,
/// temperature, pressure, visibility, rain and brightness set how thick and how high the
/// clouds are, the season and the time of day how much the warm air piles them up (cumulus
/// peak in the afternoon, flat stratus in cold or damp mornings), and the weather of the
/// date itself (a slow noise over the day count and the hour, never the same twice) moves
/// the cover and the layers up and down.
fn cloud_model(w: &::content::weather::Weather) -> CloudModel {
    let none = CloudModel {
        density: 0.0,
        layers: [[0.0; 4]; 3],
    };
    if !CLOUDS.load(std::sync::atomic::Ordering::Relaxed) {
        return none;
    }
    let (year, doy) = {
        let v = CLOUD_DAY.load(std::sync::atomic::Ordering::Relaxed);
        ((v / 1000) as i32, (v % 1000) as i32)
    };
    let hour = f32::from_bits(CLOUD_HOUR.load(std::sync::atomic::Ordering::Relaxed)).rem_euclid(24.0);
    let tau = std::f32::consts::TAU;
    let summer = 0.5 + 0.5 * (tau * (doy as f32 - 200.0) / 365.0).cos();
    let sun_heat = (0.5 + 0.5 * (tau * (hour - 15.0) / 24.0).cos()).powf(1.5);
    let tdate = doy as f64 * 24.0 + hour as f64;
    let seed = (year as u32).wrapping_mul(7919);
    let var1 = time_noise(tdate / 5.0, seed ^ 0x1111) - 0.5;
    let var2 = time_noise(tdate / 9.0, seed ^ 0x2222) - 0.5;
    let var3 = time_noise(tdate / 14.0, seed ^ 0x3333) - 0.5;

    let t = w.temp.0.clamp(-40.0, 50.0);
    let rh = rel_humidity(w);
    let humid = smooth(0.45, 1.0, rh);
    let heat = ((t - 5.0) / 25.0).clamp(0.0, 1.0);
    let cold = ((8.0 - t) / 12.0).clamp(0.0, 1.0);
    let conv = heat * sun_heat * (0.5 + 0.5 * summer);
    let low_pressure = ((1013.0 - w.pressure) / 30.0).clamp(0.0, 1.0);
    let (pk, rate) = precip_of(w);
    let rain = if pk != 0 { rate } else { 0.0 };
    let vis = w.fog.0.max(50.0);
    let v = (1.0 - vis / 30000.0).clamp(0.0, 1.0);
    let vis_term = 0.95 * v.powf(1.3);
    let dim = CustomWeather::parse(&w.path.to_string_lossy())
        .map(|c| (1.0 - c.brightness).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    // how much of the sky is covered
    let fair = 0.1 + 0.3 * conv;
    let mut density = fair + humid * (0.85 - fair) + low_pressure * 0.3 + cold * humid * 0.3;
    density += var1 * 0.5 * (1.0 - humid);
    if pk != 0 {
        density = density.max(0.6 + 0.4 * rain.sqrt());
    }
    density = density.max(vis_term).max((dim * 1.15).min(1.0));
    let density = density.clamp(0.0, 1.0);
    let thick_sky = smooth(0.7, 1.0, density);

    // the low layer: base from the dew point spread, lowered by damp, rain and haze
    let rh_c = rh.max(0.01);
    let gamma = rh_c.ln() + 17.62 * t / (243.12 + t);
    let dew = 243.12 * gamma / (17.62 - gamma);
    let spread = (t - dew).max(0.0);
    let mut base0 = 300.0 + 125.0 * spread + 25.0 * (t - 15.0).max(0.0) + 400.0 * summer * conv;
    if let Some(c) = CustomWeather::parse(&w.path.to_string_lossy()) {
        if c.cloud_base_m > 50.0 {
            base0 = c.cloud_base_m.clamp(300.0, 4000.0);
        }
    }
    base0 *= 1.0 - 0.5 * vis_term.max(rain.sqrt()).max(dim * 0.8);
    base0 *= 1.0 - 0.25 * cold;
    base0 = base0 + (700.0f32.min(base0) - base0) * thick_sky;
    let base0 = base0.clamp(250.0, 4000.0);
    let shape0 = ((0.25 + 0.75 * conv + 0.2 * heat) * (1.0 - smooth(0.55, 0.95, density)) * (1.0 - 0.8 * rain))
        .clamp(0.1, 1.0);
    let thick0 = 500.0
        + (1100.0 + 2200.0 * heat) * (1.0 - density * 0.4) * shape0
        + 700.0 * density
        + 1500.0 * rain
        + 1200.0 * dim;
    let top0 = base0 + thick0;

    // the middle and the high layer
    let mid_moist = ((rh - 0.35) / 0.5).clamp(0.0, 1.0);
    let base1 = (top0 + 1200.0).max(3000.0);
    let cover1 = (mid_moist * 0.55 * (1.0 - 0.6 * thick_sky) + low_pressure * 0.25 + var2 * 0.5).clamp(0.0, 1.0);
    let cover2 = (0.1 + 0.5 * low_pressure + 0.2 * mid_moist + 0.15 * cold + var3 * 0.6).clamp(0.0, 0.8);
    let base2 = (base1 + 2500.0).max(7000.0);
    CloudModel {
        density: if density < 0.02 { 0.0 } else { density },
        layers: [
            [base0, top0, if density < 0.02 { 0.0 } else { density }, shape0],
            [base1, base1 + 900.0, if cover1 < 0.03 { 0.0 } else { cover1 }, 0.3],
            [base2, base2 + 700.0, if cover2 < 0.03 { 0.0 } else { cover2 }, 0.5],
        ],
    }
}

pub(crate) fn cloud_layers_of(w: &::content::weather::Weather) -> [[f32; 4]; 3] {
    cloud_model(w).layers
}

pub(crate) fn clouds_of(w: &::content::weather::Weather, drift: [f32; 2]) -> (f32, [f32; 2]) {
    let density = cloud_model(w).density;
    if density <= 0.0 {
        return (0.0, [0.0; 2]);
    }
    let seed = cloud_seed();
    (density, [drift[0] + seed[0], drift[1] + seed[1]])
}

static CLOUD_DAY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub(crate) fn set_cloud_day(year: i32, day_of_year: i32) {
    CLOUD_DAY.store(
        (year as u32)
            .wrapping_mul(1000)
            .wrapping_add(day_of_year as u32),
        std::sync::atomic::Ordering::Relaxed,
    );
}

static CLOUD_HOUR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x4140_0000);

pub(crate) fn set_cloud_time(secs: f64) {
    CLOUD_HOUR.store(
        ((secs / 3600.0) as f32).to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
}

fn cloud_seed() -> [f32; 2] {
    static SESSION: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    let session = *SESSION.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() ^ (d.as_secs() as u32))
            .unwrap_or(1)
    });
    let day = CLOUD_DAY.load(std::sync::atomic::Ordering::Relaxed);
    let mut h = session ^ day.wrapping_mul(0x9e37_79b9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    [
        (h & 0xffff) as f32 / 65536.0,
        (h >> 16) as f32 / 65536.0,
    ]
}

/// The clouds' drift after `time` seconds of a steady wind (see `cloud_drift_step`).
pub(crate) fn cloud_drift_at(w: &::content::weather::Weather, time: f64) -> [f32; 2] {
    let mut d = [0.0; 2];
    cloud_drift_step(&mut d, w, time);
    d
}

pub(crate) fn cloud_drift_step(d: &mut [f32; 2], w: &::content::weather::Weather, secs: f64) {
    let (dir, speed) = (w.wind.0.to_radians() as f64, w.wind.1 as f64);
    let s = secs * speed / 70000.0;
    d[0] = (d[0] as f64 + dir.sin() * s).rem_euclid(1.0) as f32;
    d[1] = (d[1] as f64 + dir.cos() * s).rem_euclid(1.0) as f32;
}

/// How wet the roads are: rain soaks them in a few minutes, sunshine dries them in about
/// twenty. `secs` is how long this weather has been running.
pub(crate) fn road_wetness(rate: f32, secs: f64, start: f32) -> f32 {
    if rate > 0.0 {
        (start + secs as f32 * rate / 180.0).clamp(0.0, 1.0)
    } else {
        (start - secs as f32 / 1200.0).clamp(0.0, 1.0)
    }
}

/// The renderer's lighting for this weather at this moment: the daylight, then what the
/// cloud cover, the rain and the snow make of it.
pub(crate) fn weather_lighting(
    daylight: &::simulation::Daylight,
    w: &::content::weather::Weather,
    cloud_drift: [f32; 2],
    wetness: f32,
    shadows: bool,
) -> ::render::Lighting {
    let mut lighting = lights::lighting_from(daylight, w.fog.0);
    lighting.enhanced = ENHANCED.load(std::sync::atomic::Ordering::Relaxed);
    lighting.classic = CLASSIC.load(std::sync::atomic::Ordering::Relaxed);
    let (density, offset) = clouds_of(w, cloud_drift);
    lighting.cloud_density = density;
    lighting.cloud_offset = offset;
    lighting.cloud_layers = cloud_layers_of(w);
    let (kind, rate) = precip_of(w);
    lights::apply_weather(
        &mut lighting,
        density,
        kind,
        rate,
        if w.snow { 1.0 } else { 0.0 },
    );
    if let Some(custom) = CustomWeather::parse(&w.path.to_string_lossy()) {
        let k = custom.brightness;
        lighting.sun_intensity *= k;
        lighting.secondary *= k;
        lighting.ambient *= k;
        lighting.sky_color *= k;
        lighting.fog_color *= k;
    }
    lighting.wetness = wetness;
    // Omsi.exe hides the sun under an 'ovc' cloud type (the Overcast ones in clouds.cfg) and
    // draws no sun shadows below 350 m visibility
    lighting.shadows = shadows && density < 0.85 && w.fog.0 > 350.0;
    lighting.light_shadows = shadows;
    lighting
}

/// Push the weather into a vehicle's script host.
pub(crate) fn precip_of(w: &::content::weather::Weather) -> (i32, f32) {
    let kind = w.precip.first().copied().unwrap_or(0.0) as i32;
    let rate = if kind == 0 {
        0.0
    } else {
        (w.precip.get(1).copied().unwrap_or(0.0) / 255.0).clamp(0.0, 1.0)
    };
    (kind, rate)
}

pub(crate) fn apply_weather(
    v: &mut ::simulation::VehicleInstance,
    w: &::content::weather::Weather,
    wetness: f32,
) {
    let (kind, rate) = precip_of(w);
    v.host.precip_type = kind as f32;
    v.host.precip_rate = rate;
    v.host.wind = rain::weather_wind(w);
    v.host.street_cond = street_condition(w, wetness);
    v.set_var("PrecipType", kind as f32);
    v.set_var("PrecipRate", rate);
    v.host.temperature = w.temp.0;
    v.host.abs_humidity = w.temp.1;
    ::simulation::host::set_ambient_weather(w.temp.0, w.temp.1);
}

/// `OMSI_DEBUG_SOUND[=seconds]`: how often the mixer says what every sound entry of the
/// player's bus is doing, and what the environment sounds play (5 s by default).
pub(crate) fn debug_sound_every() -> Option<f32> {
    static EVERY: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *EVERY.get_or_init(|| {
        ::legacy_config::env::var("OMSI_DEBUG_SOUND")
            .ok()
            .map(|v| v.parse::<f32>().ok().filter(|s| *s > 0.0).unwrap_or(5.0))
    })
}

/// The road under the wheels as the scripts and the sound configurations read it
/// (`StreetCond`): 0 dry, 1 wet, 2 covered in snow. A snowfall (or a weather with
/// `[snowOnRoad]`) puts it into the upper half, everything else follows the water film.
pub(crate) fn street_condition(w: &::content::weather::Weather, wetness: f32) -> f32 {
    let wet = wetness.clamp(0.0, 1.0);
    if w.snow_on_road || precip_of(w).0 == 2 {
        1.0 + wet
    } else {
        wet
    }
}

/// How wet the roads are when a session starts: what this weather has already left on them
/// (`[groundwet]`, 0 … 255), and at least what the rain falling now would soak them to.
pub(crate) fn initial_wetness(w: &::content::weather::Weather) -> f32 {
    let rate = precip_of(w).1;
    let falling = if rate > 0.0 {
        (0.4 + rate).min(1.0)
    } else {
        0.0
    };
    (w.ground_wet[0] / 255.0).clamp(0.0, 1.0).max(falling)
}

/// The airports OMSI's METAR list (`Weather/ICAO.txt`) offers: (ICAO, "ICAO - name").
pub(crate) fn metar_airports(root: &Path) -> Vec<(String, String)> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, Vec<(String, String)>>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if let Some(v) = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(root)
        .cloned()
    {
        return v;
    }
    let text = std::fs::read(::mods::resolve_path(root, "Weather/ICAO.txt"))
        .map(|b| ::legacy_config::codepage::decode(&b))
        .unwrap_or_default();
    let mut v: Vec<(String, String)> = text
        .lines()
        .filter_map(|l| {
            l.split_once(" - ").map(|(c, n)| {
                (
                    c.trim().to_ascii_uppercase(),
                    format!("{} - {}", c.trim(), n.trim()),
                )
            })
        })
        .filter(|(c, _)| c.len() == 4 && c.chars().all(|x| x.is_ascii_alphabetic()))
        .collect();
    if !v.iter().any(|a| a.0 == "EDDB") {
        v.push(("EDDB".into(), "EDDB - Berlin Brandenburg".into()))
    }
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v.dedup_by(|a, b| a.0 == b.0);
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(root.to_path_buf(), v.clone());
    v
}

/// The weather of an airport's METAR report (aviationweather.gov), or a clear day when it
/// cannot be had (no network, an unknown station).
pub(crate) fn fetch_metar(icao: &str) -> ::content::weather::Weather {
    match try_metar(icao) {
        Some(w) => w,
        None => {
            log::warn!(
                "current weather at {icao}: no METAR report could be had; a clear day instead"
            );
            ::content::weather::from_metar(icao, "CAVOK")
        }
    }
}

/// The weather of an airport's METAR report, None when it cannot be had.
pub(crate) fn try_metar(icao: &str) -> Option<::content::weather::Weather> {
    // (Tegel, OMSI's Berlin default, closed in 2020: Berlin's airport now reports)
    let icao = match icao.to_ascii_uppercase().as_str() {
        "EDDT" | "EDDI" | "" => "EDDB".to_string(),
        other => other.to_string(),
    };
    let url = format!("https://aviationweather.gov/api/data/metar?ids={icao}&format=raw");
    let text = ureq::get(&url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(6)))
        .build()
        .call()
        .ok()
        .and_then(|r| r.into_body().read_to_string().ok())
        .map(|t| t.lines().next().unwrap_or("").trim().to_string())
        .filter(|t| !t.is_empty());
    let t = text?;
    log::info!("current weather at {icao}: {t}");
    let mut w = ::content::weather::from_metar(&icao, &t);
    w.path = PathBuf::from(format!("metar:{icao}"));
    Some(w)
}

/// The start of a weather text that carries a METAR report's values (not a file, not a
/// download): what a host or server with the METAR sync tells the players, who make the
/// weather from it themselves and need no sync of their own.
pub(crate) const REPORT: &str = "metar-report:";

/// `w` (made from a METAR report) as the text the session tells the players: the station and
/// the report up to its forecast, which `from_metar` does not read either.
pub(crate) fn report_wire(w: &::content::weather::Weather) -> Option<String> {
    let path = w.path.to_string_lossy();
    let icao = path.strip_prefix("metar:")?;
    let mut raw: Vec<&str> = Vec::new();
    for t in w.description.split_whitespace() {
        if matches!(
            t.trim_end_matches('='),
            "TEMPO" | "BECMG" | "NOSIG" | "RMK" | "PROB30" | "PROB40"
        ) {
            break;
        }
        raw.push(t);
    }
    let raw = raw.join(" ");
    if icao.is_empty() || raw.is_empty() {
        return None;
    }
    // (the network's weather field holds 260 characters)
    Some(format!("{REPORT}{icao} {raw}").chars().take(250).collect())
}

/// The weather of a text made by `report_wire`.
pub(crate) fn from_report(s: &str) -> Option<::content::weather::Weather> {
    let (icao, raw) = s.trim().strip_prefix(REPORT)?.split_once(' ')?;
    if !(3..=5).contains(&icao.len()) || !icao.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let mut w = ::content::weather::from_metar(icao, raw);
    w.path = PathBuf::from(format!("metar:{icao}"));
    Some(w)
}
