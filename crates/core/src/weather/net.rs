use super::metar;
use super::model::*;

const AGENT: &str = "neoOMSI weather";

fn station(icao: &str) -> String {
    match icao.trim().to_ascii_uppercase().as_str() {
        "EDDT" | "EDDI" | "" => "EDDB".to_string(),
        other => other.to_string(),
    }
}

fn get(url: &str, secs: u64, limit: u64) -> Option<String> {
    ureq::get(url)
        .header("User-Agent", AGENT)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(secs)))
        .build()
        .call()
        .ok()?
        .into_body()
        .with_config()
        .limit(limit)
        .read_to_string()
        .ok()
}

pub fn report_text(icao: &str) -> Option<String> {
    let url = format!("https://aviationweather.gov/api/data/metar?ids={}&format=raw", station(icao));
    get(&url, 6, 1 << 20)
        .map(|t| t.lines().next().unwrap_or("").trim().to_string())
        .filter(|t| !t.is_empty())
}

pub fn fetch(icao: &str) -> Option<Weather> {
    let icao = station(icao);
    let text = report_text(&icao)?;
    log::info!("current weather at {icao}: {text}");
    let mut w = metar::parse(&text);
    w.name = format!("METAR {icao}");
    w.source = Source::Metar(icao);
    Some(w)
}

pub fn fetch_or_clear(icao: &str) -> Weather {
    fetch(icao).unwrap_or_else(|| {
        log::warn!("current weather at {icao}: no METAR report could be had; a clear day instead");
        let mut w = metar::parse("ZZZZ 011200Z 00000KT CAVOK 15/05 Q1013");
        w.name = format!("METAR {}", station(icao));
        w
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationInfo {
    pub icao: String,
    pub name: String,
}

#[derive(serde::Deserialize)]
struct StationRow {
    #[serde(rename = "icaoId")]
    icao: Option<String>,
    site: Option<String>,
}

fn valid_code(code: &str, max: usize) -> bool {
    (3..=max).contains(&code.len()) && code.chars().all(|c| c.is_ascii_alphanumeric())
}

fn parse_info(icao: &str, text: &str) -> Option<StationInfo> {
    let row = serde_json::from_str::<Vec<StationRow>>(text).ok()?.into_iter().next()?;
    let (code, site) = (row.icao?, row.site?);
    (code.eq_ignore_ascii_case(icao) && !site.trim().is_empty()).then(|| StationInfo {
        icao: code.to_ascii_uppercase(),
        name: site.trim().to_string(),
    })
}

pub fn station_info(icao: &str) -> Option<StationInfo> {
    let icao = icao.trim().to_ascii_uppercase();
    if !valid_code(&icao, 4) {
        return None;
    }
    let url = format!("https://aviationweather.gov/api/data/stationinfo?ids={icao}&format=json");
    parse_info(&icao, &get(&url, 6, 1 << 20)?)
}

fn tile(lat0: i32, lon0: i32, lat1: i32, lon1: i32) -> Vec<(String, String)> {
    let url = format!("https://aviationweather.gov/api/data/stationinfo?bbox={lat0},{lon0},{lat1},{lon1}&format=json");
    for _ in 0..2 {
        let Some(text) = get(&url, 40, 256 << 20) else { continue };
        let Ok(rows) = serde_json::from_str::<Vec<StationRow>>(&text) else { continue };
        return rows
            .into_iter()
            .filter_map(|r| {
                let code = r.icao?.trim().to_ascii_uppercase();
                let name = r.site.unwrap_or_default().trim().to_string();
                valid_code(&code, 4).then(|| {
                    let label = if name.is_empty() { code.clone() } else { format!("{code} - {name}") };
                    (code, label)
                })
            })
            .collect();
    }
    Vec::new()
}

pub fn station_list() -> Vec<(String, String)> {
    let mut jobs = Vec::new();
    for lat in (-90..90).step_by(30) {
        for lon in (-180..180).step_by(60) {
            jobs.push(std::thread::spawn(move || tile(lat, lon, lat + 30, lon + 60)));
        }
    }
    let mut out: Vec<(String, String)> = jobs.into_iter().flat_map(|j| j.join().unwrap_or_default()).collect();
    out.sort();
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_station_row_names_the_airport() {
        let info = super::parse_info("EGNX", r#"[{"icaoId":"EGNX","site":"Nottingham/E Midlands Arpt"}]"#).unwrap();
        assert_eq!(info.name, "Nottingham/E Midlands Arpt");
    }
}
