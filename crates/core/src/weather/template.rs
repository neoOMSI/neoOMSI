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

struct Stock {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    metar: &'static str,
    months: Option<(u32, u32)>,
    wet: Option<f32>,
    snow: bool,
    next: &'static [&'static str],
}

const STOCK: &[Stock] = &[
    Stock { id: "clear", name: "Clear", description: "Not a cloud in the sky.", metar: "ZZZZ 011200Z 00000KT CAVOK 18/08 Q1020", months: None, wet: None, snow: false, next: &["fair", "hot", "haze"] },
    Stock { id: "fair", name: "Fair", description: "A few fair-weather clouds.", metar: "ZZZZ 011200Z 27006KT 9999 FEW040 20/10 Q1018", months: None, wet: None, snow: false, next: &["clear", "partly_cloudy", "haze"] },
    Stock { id: "partly_cloudy", name: "Partly cloudy", description: "Scattered cumulus.", metar: "ZZZZ 011200Z 25009KT 9999 SCT030 SCT080 18/09 Q1015", months: None, wet: None, snow: false, next: &["fair", "cloudy", "showers"] },
    Stock { id: "cloudy", name: "Cloudy", description: "A broken cloud deck.", metar: "ZZZZ 011200Z 24010KT 9999 BKN028 15/09 Q1010", months: None, wet: None, snow: false, next: &["partly_cloudy", "overcast", "light_rain", "mist"] },
    Stock { id: "overcast", name: "Overcast", description: "A closed grey sky.", metar: "ZZZZ 011200Z 24008KT 8000 OVC018 11/08 Q1008", months: None, wet: None, snow: false, next: &["cloudy", "drizzle", "light_rain", "mist"] },
    Stock { id: "haze", name: "Haze", description: "Dry, hazy air.", metar: "ZZZZ 011200Z 18004KT 5000 HZ SKC 27/14 Q1017", months: Some((5, 9)), wet: None, snow: false, next: &["clear", "hot", "fair"] },
    Stock { id: "hot", name: "Hot", description: "Cloudless and hot.", metar: "ZZZZ 011200Z 20005KT CAVOK 34/12 Q1011", months: Some((6, 8)), wet: None, snow: false, next: &["clear", "haze", "thunderstorm"] },
    Stock { id: "mist", name: "Mist", description: "Damp air, short sight.", metar: "ZZZZ 011200Z 00000KT 3000 BR BKN004 09/08 Q1019", months: None, wet: Some(0.25), snow: false, next: &["fog", "overcast", "fair"] },
    Stock { id: "fog", name: "Fog", description: "Dense fog on a cold, still morning.", metar: "ZZZZ 011200Z 00000KT 0300 FG VV002 05/05 Q1024", months: Some((9, 3)), wet: Some(0.3), snow: false, next: &["mist", "overcast"] },
    Stock { id: "drizzle", name: "Drizzle", description: "Fine, steady drizzle.", metar: "ZZZZ 011200Z 22007KT 3500 -DZ BR OVC009 11/10 Q1006", months: None, wet: None, snow: false, next: &["light_rain", "overcast", "mist"] },
    Stock { id: "light_rain", name: "Light rain", description: "Light rain from a low deck.", metar: "ZZZZ 011200Z 23008KT 6000 -RA BKN014 OVC025 13/11 Q1003", months: None, wet: None, snow: false, next: &["rain", "drizzle", "cloudy"] },
    Stock { id: "rain", name: "Rain", description: "Steady rain.", metar: "ZZZZ 011200Z 23012KT 4000 RA OVC012 12/11 Q0998", months: None, wet: None, snow: false, next: &["heavy_rain", "light_rain", "showers"] },
    Stock { id: "heavy_rain", name: "Heavy rain", description: "A downpour with a gusty wind.", metar: "ZZZZ 011200Z 22014G26KT 1800 +RA OVC008 13/12 Q0992", months: None, wet: None, snow: false, next: &["rain", "thunderstorm"] },
    Stock { id: "showers", name: "Showers", description: "Showers between the sun: it rains in places only.", metar: "ZZZZ 011200Z 26012G22KT 9000 SHRA SCT025 BKN040CB 14/08 Q1005", months: None, wet: None, snow: false, next: &["partly_cloudy", "rain", "thunderstorm", "cloudy"] },
    Stock { id: "showers_nearby", name: "Showers nearby", description: "Dry here, with showers in the vicinity.", metar: "ZZZZ 011200Z 27010KT 9999 VCSH SCT028 BKN045CB 16/09 Q1008", months: None, wet: None, snow: false, next: &["showers", "partly_cloudy"] },
    Stock { id: "thunderstorm", name: "Thunderstorm", description: "Cumulonimbus and a heavy downpour in cells.", metar: "ZZZZ 011200Z 20012G32KT 2500 +TSRA BKN018CB 23/20 Q1003", months: Some((5, 9)), wet: None, snow: false, next: &["heavy_rain", "showers", "cloudy"] },
    Stock { id: "light_snow", name: "Light snow", description: "Light snowfall.", metar: "ZZZZ 011200Z 31006KT 4000 -SN OVC020 M01/M03 Q1011", months: Some((11, 3)), wet: None, snow: true, next: &["snow", "overcast", "frost"] },
    Stock { id: "snow", name: "Snow", description: "Steady snow, settling on the road.", metar: "ZZZZ 011200Z 32010KT 1500 SN OVC012 M03/M05 Q1006", months: Some((11, 3)), wet: None, snow: true, next: &["light_snow", "blizzard"] },
    Stock { id: "blizzard", name: "Blizzard", description: "Heavy snow, a gale and no sight.", metar: "ZZZZ 011200Z 36020G35KT 0400 +SN VV003 M07/M09 Q0996", months: Some((12, 2)), wet: None, snow: true, next: &["snow"] },
    Stock { id: "frost", name: "Frost", description: "A clear, hard frost.", metar: "ZZZZ 011200Z 00000KT CAVOK M08/M12 Q1031", months: Some((11, 3)), wet: None, snow: true, next: &["clear", "fog", "light_snow"] },
];

fn stock() -> Vec<Template> {
    STOCK
        .iter()
        .map(|s| Template {
            id: s.id.into(),
            name: s.name.into(),
            description: s.description.into(),
            metar: s.metar.into(),
            brightness: None,
            ground_wet: s.wet,
            snow_cover: s.snow.then_some(true),
            snow_on_road: None,
            temp_c: None,
            months: s.months,
            next: s.next.iter().map(|n| n.to_string()).collect(),
            cycle: true,
            stock: true,
        })
        .collect()
}

fn read_user(file: &str, known: &[Template]) -> Option<Template> {
    let path: PathBuf = ::mods::find_in_roots(&format!("Weather/Templates/{file}"))?.1;
    let bytes = ::legacy_config::vfs::read(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&bytes)).ok()?;
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
                assert!(STOCK.iter().any(|s| s.id == n), "{} -> {n}", t.id);
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
