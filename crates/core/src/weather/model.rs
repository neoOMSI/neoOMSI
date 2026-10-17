#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Haze {
    #[default]
    None,
    Haze,
    Mist,
    Fog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloudKind {
    #[default]
    Plain,
    Towering,
    Storm,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    pub cover: f32,
    pub base_m: f32,
    pub kind: CloudKind,
}

impl Layer {
    pub fn code(&self) -> &'static str {
        match self.cover {
            c if c >= 0.875 => "OVC",
            c if c >= 0.6 => "BKN",
            c if c >= 0.3 => "SCT",
            _ => "FEW",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrecipKind {
    #[default]
    None,
    Drizzle,
    Rain,
    Snow,
    Sleet,
    Hail,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Precip {
    pub kind: PrecipKind,
    pub intensity: f32,
    pub showers: bool,
    pub thunder: bool,
    pub vicinity: bool,
}

impl Precip {
    pub fn is_falling(&self) -> bool {
        self.kind != PrecipKind::None && self.intensity > 0.0
    }

    pub fn sim_kind(&self) -> i32 {
        match self.kind {
            PrecipKind::None => 0,
            PrecipKind::Snow | PrecipKind::Sleet => 2,
            _ => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Source {
    #[default]
    Custom,
    Template(String),
    Metar(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Weather {
    pub name: String,
    pub description: String,
    pub source: Source,
    pub visibility_m: f32,
    pub haze: Haze,
    pub wind_dir: f32,
    pub wind_speed: f32,
    pub wind_gust: f32,
    pub temp_c: f32,
    pub dew_c: f32,
    pub pressure_hpa: f32,
    pub clouds: Vec<Layer>,
    pub precip: Precip,
    pub snow_cover: bool,
    pub snow_on_road: bool,
    pub ground_wet: f32,
    pub brightness: f32,
}

impl Default for Weather {
    fn default() -> Self {
        Weather {
            name: String::new(),
            description: String::new(),
            source: Source::Custom,
            visibility_m: 50_000.0,
            haze: Haze::None,
            wind_dir: 0.0,
            wind_speed: 0.0,
            wind_gust: 0.0,
            temp_c: 15.0,
            dew_c: 5.0,
            pressure_hpa: 1013.0,
            clouds: Vec::new(),
            precip: Precip::default(),
            snow_cover: false,
            snow_on_road: false,
            ground_wet: 0.0,
            brightness: 1.0,
        }
    }
}

pub fn saturation_vapour_hpa(temp_c: f32) -> f32 {
    6.112 * ((17.67 * temp_c) / (temp_c + 243.5)).exp()
}

pub fn dew_point_c(temp_c: f32, rel_humidity_pct: f32) -> f32 {
    let g = (rel_humidity_pct.clamp(0.1, 100.0) / 100.0).ln() + 17.67 * temp_c / (243.5 + temp_c);
    243.5 * g / (17.67 - g)
}

pub fn abs_humidity(temp_c: f32, rel_humidity_pct: f32) -> f32 {
    let e = saturation_vapour_hpa(temp_c) * rel_humidity_pct.clamp(0.0, 100.0) / 100.0;
    (216.7 * e / (temp_c + 273.15).max(1.0)).max(0.0)
}

impl Weather {
    pub fn rel_humidity(&self) -> f32 {
        let e = saturation_vapour_hpa(self.dew_c.min(self.temp_c));
        (e / saturation_vapour_hpa(self.temp_c).max(0.001) * 100.0).clamp(0.0, 100.0)
    }

    pub fn abs_humidity(&self) -> f32 {
        abs_humidity(self.temp_c, self.rel_humidity())
    }

    pub fn set_rel_humidity(&mut self, pct: f32) {
        self.dew_c = dew_point_c(self.temp_c, pct).min(self.temp_c);
    }

    pub fn cover(&self) -> f32 {
        1.0 - self
            .clouds
            .iter()
            .fold(1.0, |rest, l| rest * (1.0 - l.cover.clamp(0.0, 1.0)))
    }

    pub fn ceiling_m(&self) -> Option<f32> {
        self.clouds
            .iter()
            .filter(|l| l.cover >= 0.6)
            .map(|l| l.base_m)
            .reduce(f32::min)
    }

    pub fn falling(&self) -> bool {
        self.precip.is_falling() && !self.precip.vicinity
    }

    pub fn is_snowy(&self) -> bool {
        matches!(self.precip.kind, PrecipKind::Snow | PrecipKind::Sleet) && self.precip.is_falling()
    }

    pub fn snowy_ground(&self) -> bool {
        self.snow_cover || self.is_snowy()
    }

    pub fn simple_cloud_index(&self) -> usize {
        match self.cover() {
            c if c < 0.06 => 0,
            c if c < 0.35 => 1,
            c if c < 0.65 => 2,
            c if c < 0.9 => 3,
            _ => 4,
        }
    }

    pub fn set_simple_cloud(&mut self, index: usize, base_m: f32) {
        self.clouds.clear();
        let cover = [0.0, 0.25, 0.5, 0.78, 1.0][index.min(4)];
        if cover > 0.0 {
            self.clouds.push(Layer {
                cover,
                base_m: base_m.clamp(50.0, 6000.0),
                kind: CloudKind::Plain,
            });
        }
    }

    pub fn simple_precip_index(&self) -> usize {
        match self.precip.kind {
            PrecipKind::None => 0,
            PrecipKind::Snow | PrecipKind::Sleet => 2,
            _ => 1,
        }
    }

    pub fn set_simple_precip(&mut self, index: usize) {
        let keep = self.precip.intensity.max(0.4);
        self.precip = Precip::default();
        match index {
            1 => {
                self.precip.kind = PrecipKind::Rain;
                self.precip.intensity = keep;
            }
            2 => {
                self.precip.kind = PrecipKind::Snow;
                self.precip.intensity = keep;
                self.snow_cover = true;
            }
            _ => {}
        }
        if index != 0 && self.cover() < 0.85 {
            self.set_simple_cloud(4, self.clouds.first().map_or(600.0, |l| l.base_m));
        }
    }

    pub fn set_precip_amount(&mut self, v: f32) {
        if v <= 0.0 {
            self.precip = Precip::default();
            return;
        }
        if self.precip.kind == PrecipKind::None {
            self.set_simple_precip(1);
        }
        self.precip.intensity = v.clamp(0.0, 1.0);
    }

    pub fn lerp(a: &Weather, b: &Weather, k: f32) -> Weather {
        let k = k.clamp(0.0, 1.0);
        let m = |x: f32, y: f32| x + (y - x) * k;
        let late = k >= 0.5;
        let vis = (a.visibility_m.max(1.0).ln() * (1.0 - k) + b.visibility_m.max(1.0).ln() * k).exp();
        let n = a.clouds.len().max(b.clouds.len());
        let clouds = (0..n)
            .map(|i| {
                let la = a.clouds.get(i).copied();
                let lb = b.clouds.get(i).copied();
                let (ca, ba) = la.map_or((0.0, lb.map_or(1000.0, |l| l.base_m)), |l| (l.cover, l.base_m));
                let (cb, bb) = lb.map_or((0.0, ba), |l| (l.cover, l.base_m));
                let pick = if late { lb.or(la) } else { la.or(lb) };
                Layer {
                    cover: m(ca, cb),
                    base_m: m(ba, bb),
                    kind: pick.map_or(CloudKind::Plain, |l| l.kind),
                }
            })
            .filter(|l| l.cover > 0.01)
            .collect();
        let (pa, pb) = (a.precip, b.precip);
        let dominant = if pa.kind == PrecipKind::None {
            pb
        } else if pb.kind == PrecipKind::None {
            pa
        } else if late {
            pb
        } else {
            pa
        };
        let precip = Precip {
            intensity: m(
                if pa.kind == PrecipKind::None { 0.0 } else { pa.intensity },
                if pb.kind == PrecipKind::None { 0.0 } else { pb.intensity },
            ),
            ..dominant
        };
        let precip = if precip.intensity < 0.01 {
            Precip::default()
        } else {
            precip
        };
        let dir_delta = (b.wind_dir - a.wind_dir + 540.0).rem_euclid(360.0) - 180.0;
        Weather {
            name: if late { b.name.clone() } else { a.name.clone() },
            description: b.description.clone(),
            source: if late { b.source.clone() } else { a.source.clone() },
            visibility_m: vis,
            haze: if late { b.haze } else { a.haze },
            wind_dir: (a.wind_dir + dir_delta * k).rem_euclid(360.0),
            wind_speed: m(a.wind_speed, b.wind_speed),
            wind_gust: m(a.wind_gust, b.wind_gust),
            temp_c: m(a.temp_c, b.temp_c),
            dew_c: m(a.dew_c, b.dew_c),
            pressure_hpa: m(a.pressure_hpa, b.pressure_hpa),
            clouds,
            precip,
            snow_cover: if late { b.snow_cover } else { a.snow_cover },
            snow_on_road: if late { b.snow_on_road } else { a.snow_on_road },
            ground_wet: m(a.ground_wet, b.ground_wet),
            brightness: m(a.brightness, b.brightness),
        }
    }
}

pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let k = ((x - a) / (b - a)).clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}
