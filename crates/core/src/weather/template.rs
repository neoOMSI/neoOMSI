use super::metar;
use super::model::*;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub description: String,
    pub metar: String,
    pub brightness: Option<f32>,
    pub ground_wet: Option<f32>,
    pub snow_cover: Option<bool>,
    pub snow_on_road: Option<bool>,
    pub temp_c: Option<f32>,
    pub months: Option<(u32, u32)>,
    pub next: Vec<String>,
    pub cycle: bool,
    pub stock: bool,
}

impl Template {
    pub fn weather(&self) -> Weather {
        let mut w = metar::parse(&self.metar);
        w.name = self.name.clone();
        w.description = self.description.clone();
        w.source = Source::Template(self.id.clone());
        if let Some(t) = self.temp_c {
            let rh = w.rel_humidity();
            w.temp_c = t;
            w.set_rel_humidity(rh);
        }
        if let Some(b) = self.brightness {
            w.brightness = b.clamp(0.0, 1.5);
        }
        if let Some(v) = self.ground_wet {
            w.ground_wet = v.clamp(0.0, 1.0);
        }
        if let Some(v) = self.snow_cover {
            w.snow_cover = v;
        }
        if let Some(v) = self.snow_on_road {
            w.snow_on_road = v;
        }
        w
    }

    pub fn in_season(&self, month: u32) -> bool {
        match self.months {
            None => true,
            Some((a, b)) if a <= b => (a..=b).contains(&month),
            Some((a, b)) => month >= a || month <= b,
        }
    }
}

pub fn normalize_id(s: &str) -> String {
    let s = s.replace('\\', "/");
    let file = s.rsplit('/').next().unwrap_or("");
    let stem = file.rsplit_once('.').map_or(file, |x| x.0);
    stem.trim_start_matches('#')
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
}

const STOCK_FILES: &[(&str, &str)] = &[
    ("clear.json", include_str!("../../../../assets/weather/templates/clear.json")),
    ("fair.json", include_str!("../../../../assets/weather/templates/fair.json")),
    ("partly_cloudy.json", include_str!("../../../../assets/weather/templates/partly_cloudy.json")),
    ("breezy.json", include_str!("../../../../assets/weather/templates/breezy.json")),
    ("cloudy.json", include_str!("../../../../assets/weather/templates/cloudy.json")),
    ("overcast.json", include_str!("../../../../assets/weather/templates/overcast.json")),
    ("haze.json", include_str!("../../../../assets/weather/templates/haze.json")),
    ("hot.json", include_str!("../../../../assets/weather/templates/hot.json")),
    ("buildup.json", include_str!("../../../../assets/weather/templates/buildup.json")),
    ("mist.json", include_str!("../../../../assets/weather/templates/mist.json")),
    ("fog.json", include_str!("../../../../assets/weather/templates/fog.json")),
    ("freezing_fog.json", include_str!("../../../../assets/weather/templates/freezing_fog.json")),
    ("drizzle.json", include_str!("../../../../assets/weather/templates/drizzle.json")),
    ("light_rain.json", include_str!("../../../../assets/weather/templates/light_rain.json")),
    ("rain.json", include_str!("../../../../assets/weather/templates/rain.json")),
    ("heavy_rain.json", include_str!("../../../../assets/weather/templates/heavy_rain.json")),
    ("showers.json", include_str!("../../../../assets/weather/templates/showers.json")),
    ("showers_nearby.json", include_str!("../../../../assets/weather/templates/showers_nearby.json")),
    ("thunderstorm.json", include_str!("../../../../assets/weather/templates/thunderstorm.json")),
    ("dark_storm.json", include_str!("../../../../assets/weather/templates/dark_storm.json")),
    ("gale.json", include_str!("../../../../assets/weather/templates/gale.json")),
    ("hail.json", include_str!("../../../../assets/weather/templates/hail.json")),
    ("sleet.json", include_str!("../../../../assets/weather/templates/sleet.json")),
    ("light_snow.json", include_str!("../../../../assets/weather/templates/light_snow.json")),
    ("snow.json", include_str!("../../../../assets/weather/templates/snow.json")),
    ("blizzard.json", include_str!("../../../../assets/weather/templates/blizzard.json")),
    ("frost.json", include_str!("../../../../assets/weather/templates/frost.json")),
];

fn stock() -> Vec<Template> {
    STOCK_FILES
        .iter()
        .filter_map(|(file, text)| {
            parse_json(file, text, &[]).map(|t| Template { stock: true, ..t })
        })
        .collect()
}

fn read_user(file: &str, known: &[Template]) -> Option<Template> {
    let path: PathBuf = ::mods::find_in_roots(&format!("Weather/Templates/{file}"))?.1;
    let bytes = ::legacy_config::vfs::read(&path).ok()?;
    parse_json(file, &String::from_utf8_lossy(&bytes), known)
}

fn parse_json(file: &str, json: &str, known: &[Template]) -> Option<Template> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let id = normalize_id(file);
    let parent = v
        .get("extends")
        .and_then(|e| e.as_str())
        .map(normalize_id)
        .and_then(|p| known.iter().find(|t| t.id == p).cloned());
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).map(|x| x as f32);
    let flag = |k: &str| v.get(k).and_then(|x| x.as_bool());
    let metar = text("metar").or_else(|| parent.as_ref().map(|p| p.metar.clone()))?;
    let months = v.get("months").and_then(|m| m.as_array()).and_then(|m| {
        Some((m.first()?.as_u64()? as u32, m.get(1)?.as_u64()? as u32))
    });
    let next = v
        .get("next")
        .and_then(|n| n.as_array())
        .map(|n| n.iter().filter_map(|x| x.as_str().map(normalize_id)).collect())
        .or_else(|| parent.as_ref().map(|p| p.next.clone()))
        .unwrap_or_default();
    Some(Template {
        name: text("name").unwrap_or_else(|| id.replace('_', " ")),
        description: text("description").or_else(|| parent.as_ref().map(|p| p.description.clone())).unwrap_or_default(),
        id,
        metar,
        brightness: num("brightness").or(parent.as_ref().and_then(|p| p.brightness)),
        ground_wet: num("ground_wet").or(parent.as_ref().and_then(|p| p.ground_wet)),
        snow_cover: flag("snow_cover").or(parent.as_ref().and_then(|p| p.snow_cover)),
        snow_on_road: flag("snow_on_road").or(parent.as_ref().and_then(|p| p.snow_on_road)),
        temp_c: num("temp_c").or(parent.as_ref().and_then(|p| p.temp_c)),
        months: months.or(parent.as_ref().and_then(|p| p.months)),
        next,
        cycle: flag("cycle").unwrap_or(true),
        stock: false,
    })
}

pub fn library() -> Vec<Template> {
    let mut all = stock();
    let mut files: Vec<String> = ::mods::read_dir_merged("Weather/Templates")
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")))
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();
    files.sort();
    files.dedup();
    for f in files {
        if let Some(t) = read_user(&f, &all) {
            match all.iter().position(|x| x.id == t.id) {
                Some(i) => all[i] = t,
                None => all.push(t),
            }
        }
    }
    all
}

pub fn find(id: &str) -> Option<Template> {
    let id = normalize_id(id);
    library().into_iter().find(|t| t.id == id)
}

pub fn save(w: &Weather, name: &str) -> Result<PathBuf, String> {
    let id = normalize_id(name);
    if id.is_empty() {
        return Err("Give the weather a name".into());
    }
    let dir = crate::startup::content_dir()
        .ok_or_else(|| "no content folder".to_string())?
        .join("Weather")
        .join("Templates");
    let value = serde_json::json!({
        "name": name.trim(),
        "description": w.description,
        "metar": metar::encode(w, ""),
        "brightness": w.brightness,
        "ground_wet": w.ground_wet,
        "snow_cover": w.snow_cover,
        "snow_on_road": w.snow_on_road,
    });
    let path = dir.join(format!("{id}.json"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stock_template_reads_as_what_it_says() {
        for t in stock() {
            let w = t.weather();
            assert_eq!(w.source, Source::Template(t.id.clone()));
            for n in &t.next {
                assert!(stock().iter().any(|s| &s.id == n), "{} -> {n}", t.id);
            }
        }
        let rain = stock().into_iter().find(|t| t.id == "rain").unwrap().weather();
        assert_eq!(rain.precip.kind, PrecipKind::Rain);
        let near = stock().into_iter().find(|t| t.id == "showers_nearby").unwrap().weather();
        assert!(near.precip.vicinity && !near.falling());
    }

    #[test]
    fn old_file_names_find_their_template() {
        assert_eq!(normalize_id("Weather/#CAVOK.owt"), "cavok");
        assert_eq!(normalize_id("Heavy Rain"), "heavy_rain");
        assert!(Template { months: Some((11, 3)), ..stock()[0].clone() }.in_season(1));
    }
}
