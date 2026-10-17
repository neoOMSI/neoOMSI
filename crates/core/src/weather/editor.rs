use super::model::*;

pub const CUSTOM_CLOUDS: [&str; 5] = ["No clouds", "Cumulus 1", "Cumulus 2", "Cumulus 3", "Overcast 1"];
pub const CUSTOM_PRECIP: [&str; 3] = ["None", "Rain", "Snow"];

#[derive(Debug, Clone, PartialEq)]
pub struct CustomWeather {
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
        CustomWeather {
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

pub trait EditorSource {
    fn to_editor(&self, brightness: f32, wetness: f32) -> CustomWeather;
}

impl EditorSource for Weather {
    fn to_editor(&self, brightness: f32, wetness: f32) -> CustomWeather {
        let mut c = CustomWeather {
            visibility_m: self.visibility_m,
            brightness,
            wind_dir: self.wind_dir,
            wind_speed: self.wind_speed,
            temp_c: self.temp_c,
            humidity: self.rel_humidity(),
            pressure: self.pressure_hpa,
            cloud: self.simple_cloud_index(),
            cloud_base_m: self.clouds.first().map_or(50.0, |l| l.base_m),
            precip: self.simple_precip_index() as i32,
            precip_intensity: if self.precip.is_falling() {
                (self.precip.intensity * 255.0).round()
            } else {
                32.0
            },
            road_wetness: wetness,
            snow_cover: self.snow_cover,
            snow_on_road: self.snow_on_road,
        };
        c.normalize();
        c
    }
}

impl EditorSource for ::content::weather::Weather {
    fn to_editor(&self, brightness: f32, wetness: f32) -> CustomWeather {
        let kind = self.clouds.0.trim().to_ascii_lowercase();
        let cloud = ["cumulus 1", "cumulus 2", "cumulus 3", "overcast"]
            .iter()
            .position(|k| kind.starts_with(k))
            .map_or(0, |i| i + 1);
        let mut c = CustomWeather {
            visibility_m: self.fog.0,
            brightness,
            wind_dir: self.wind.0,
            wind_speed: self.wind.1,
            temp_c: self.temp.0,
            humidity: relative_from_absolute(self.temp.0, self.temp.1),
            pressure: if self.pressure > 0.0 { self.pressure } else { 1013.0 },
            cloud,
            cloud_base_m: self.clouds.1.max(50.0),
            precip: self.precip.first().copied().unwrap_or(0.0).round() as i32,
            precip_intensity: self.precip.get(1).copied().unwrap_or(32.0),
            road_wetness: wetness,
            snow_cover: self.snow,
            snow_on_road: self.snow_on_road,
        };
        c.normalize();
        c
    }
}

fn relative_from_absolute(temp_c: f32, absolute: f32) -> f32 {
    let vapour = absolute.max(0.0) * (temp_c + 273.15).max(1.0) / 216.7;
    (vapour / saturation_vapour_hpa(temp_c).max(0.001) * 100.0).clamp(0.0, 100.0)
}

impl CustomWeather {
    pub fn from_weather<W: EditorSource>(w: &W, brightness: f32, wetness: f32) -> CustomWeather {
        w.to_editor(brightness, wetness)
    }

    pub fn normalize(&mut self) {
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

    pub fn parse(text: &str) -> Option<CustomWeather> {
        let body = text.strip_prefix("custom:").or_else(|| text.strip_prefix("CUSTOM:"))?;
        let mut c = CustomWeather::default();
        for part in body.split(';') {
            let Some((key, value)) = part.split_once('=') else { continue };
            let Some(n) = value.trim().parse::<f32>().ok() else { continue };
            match key.trim().to_ascii_lowercase().as_str() {
                "vis" => c.visibility_m = n,
                "br" => c.brightness = n,
                "wd" => c.wind_dir = n,
                "ws" => c.wind_speed = n,
                "t" => c.temp_c = n,
                "rh" => c.humidity = n,
                "p" => c.pressure = n,
                "c" => c.cloud = n.round().max(0.0) as usize,
                "cb" => c.cloud_base_m = n,
                "pt" => c.precip = n.round() as i32,
                "pi" => c.precip_intensity = n,
                "wet" => c.road_wetness = n,
                "snow" => c.snow_cover = n >= 0.5,
                "snowroad" => c.snow_on_road = n >= 0.5,
                _ => {}
            }
        }
        c.normalize();
        Some(c)
    }

    pub fn encode(&self) -> String {
        let mut c = self.clone();
        c.normalize();
        format!(
            "custom:vis={:.0};br={:.2};wd={:.0};ws={:.1};t={:.1};rh={:.0};p={:.0};c={};cb={:.0};pt={};pi={:.0};wet={:.2};snow={};snowroad={}",
            c.visibility_m, c.brightness, c.wind_dir, c.wind_speed, c.temp_c, c.humidity, c.pressure, c.cloud,
            c.cloud_base_m, c.precip, c.precip_intensity, c.road_wetness, c.snow_cover as u8, c.snow_on_road as u8
        )
    }

    pub fn to_weather(&self) -> Weather {
        let mut c = self.clone();
        c.normalize();
        let mut w = Weather {
            name: "Custom weather".into(),
            description: "User-defined weather".into(),
            source: Source::Custom,
            visibility_m: c.visibility_m,
            haze: match c.visibility_m {
                v if v < 1000.0 => Haze::Fog,
                v if v < 5000.0 => Haze::Mist,
                _ => Haze::None,
            },
            wind_dir: c.wind_dir,
            wind_speed: c.wind_speed,
            temp_c: c.temp_c,
            pressure_hpa: c.pressure,
            snow_cover: c.snow_cover,
            snow_on_road: c.snow_on_road,
            ground_wet: c.road_wetness,
            brightness: c.brightness,
            ..Default::default()
        };
        w.set_rel_humidity(c.humidity);
        w.set_simple_cloud(c.cloud, c.cloud_base_m);
        let intensity = (c.precip_intensity / 255.0).clamp(0.0, 1.0);
        match c.precip {
            1 | 2 if intensity > 0.0 => {
                w.precip = Precip {
                    kind: if c.precip == 1 { PrecipKind::Rain } else { PrecipKind::Snow },
                    intensity,
                    ..Default::default()
                };
                if w.cover() < 0.85 {
                    w.set_simple_cloud(4, c.cloud_base_m);
                }
            }
            _ => {}
        }
        w
    }
}

pub fn custom_weather(text: Option<&str>) -> Option<CustomWeather> {
    text.and_then(CustomWeather::parse)
}

pub fn dew_point(temp_c: f32, rel_humidity_pct: f32) -> f32 {
    dew_point_c(temp_c, rel_humidity_pct)
}
