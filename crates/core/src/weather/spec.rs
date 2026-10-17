use super::editor::CustomWeather;
use super::model::*;
use super::{metar, net, template};
use crate::cli::Args;

pub const CYCLE: &str = "cycle";
pub const REPORT: &str = "metar-report:";
pub const RAW: &str = "wx:";
pub const TEMPLATE: &str = "template:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetarMode {
    Once,
    Sync,
    Legacy,
}

pub fn metar_mode(spec: &str) -> Option<(MetarMode, &str)> {
    let s = spec.trim();
    [("metar-once:", MetarMode::Once), ("metar-sync:", MetarMode::Sync), ("metar:", MetarMode::Legacy)]
        .into_iter()
        .find_map(|(p, mode)| {
            (s.len() >= p.len() && s.is_char_boundary(p.len()) && s[..p.len()].eq_ignore_ascii_case(p))
                .then(|| (mode, &s[p.len()..]))
        })
}

pub fn is_cycle(spec: Option<&str>) -> bool {
    spec.is_some_and(|s| s.trim().eq_ignore_ascii_case(CYCLE))
}

pub fn template_spec(id: &str) -> String {
    format!("{TEMPLATE}{id}")
}

pub fn wire(w: &Weather) -> Option<String> {
    if let Source::Metar(icao) = &w.source {
        if (3..=5).contains(&icao.len()) && icao.chars().all(|c| c.is_ascii_alphanumeric()) {
            let mut raw: Vec<&str> = Vec::new();
            for t in w.description.split_whitespace() {
                if metar::is_trend(t.trim_end_matches('=')) {
                    break;
                }
                raw.push(t);
            }
            if !raw.is_empty() {
                return Some(format!("{REPORT}{icao} {}", raw.join(" ")).chars().take(250).collect());
            }
        }
    }
    let clean = |s: &str| s.replace(['|', ';', '='], " ");
    Some(
        format!(
            "{RAW}{}|b={:.2};w={:.2};s={};r={};n={}",
            metar::encode(w, ""),
            w.brightness,
            w.ground_wet,
            w.snow_cover as u8,
            w.snow_on_road as u8,
            clean(&w.name)
        )
            .chars()
            .take(250)
            .collect(),
    )
}

pub fn from_wire(s: &str) -> Option<Weather> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix(REPORT) {
        let (icao, raw) = rest.split_once(' ')?;
        if !(3..=5).contains(&icao.len()) || !icao.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        let mut w = metar::parse(raw);
        w.name = format!("METAR {icao}");
        w.source = Source::Metar(icao.to_string());
        return Some(w);
    }
    let rest = s.strip_prefix(RAW)?;
    let (report, extras) = rest.split_once('|').unwrap_or((rest, ""));
    let mut w = metar::parse(report);
    w.source = Source::Custom;
    w.name = "Weather".into();
    for kv in extras.split(';') {
        let Some((k, v)) = kv.split_once('=') else { continue };
        let n = v.trim().parse::<f32>().ok();
        match (k.trim(), n) {
            ("b", Some(n)) => w.brightness = n.clamp(0.0, 1.5),
            ("w", Some(n)) => w.ground_wet = n.clamp(0.0, 1.0),
            ("s", Some(n)) => w.snow_cover = n >= 0.5,
            ("r", Some(n)) => w.snow_on_road = n >= 0.5,
            ("n", _) if !v.trim().is_empty() => w.name = v.trim().to_string(),
            _ => {}
        }
    }
    Some(w)
}

pub fn is_portable(spec: &str) -> bool {
    let s = spec.trim();
    s.starts_with(REPORT)
        || s.starts_with(RAW)
        || CustomWeather::parse(s).is_some()
        || template::find(s.strip_prefix(TEMPLATE).unwrap_or(s)).is_some()
}

pub fn is_readable(spec: &str) -> bool {
    let s = spec.trim();
    if s.starts_with(REPORT) {
        return from_wire(s).is_some();
    }
    is_portable(s)
}

fn from_template(id: &str) -> Weather {
    let id = template::normalize_id(id);
    let id = match id.as_str() {
        "cavok" | "clear_sky" => "clear".to_string(),
        _ => id,
    };
    template::find(&id)
        .or_else(|| template::find("fair"))
        .map(|t| t.weather())
        .unwrap_or_default()
}

pub fn resolve(spec: &str, args: &Args) -> Weather {
    let s = spec.trim();
    if let Some(c) = CustomWeather::parse(s) {
        return c.to_weather();
    }
    if s.starts_with(REPORT) || s.starts_with(RAW) {
        return from_wire(s).unwrap_or_else(|| from_template("clear"));
    }
    if let Some((_, icao)) = metar_mode(s) {
        let station = if icao.trim().is_empty() {
            crate::launcher::drive::nearest_airport(&args.root.to_string_lossy(), &args.map)
        } else {
            icao.trim().to_ascii_uppercase()
        };
        return net::fetch_or_clear(&station);
    }
    from_template(s.strip_prefix(TEMPLATE).unwrap_or(s))
}

pub fn load(args: &Args) -> Weather {
    let w = match args.weather.as_deref().filter(|s| !is_cycle(Some(s)) && !s.trim().is_empty()) {
        Some(spec) => resolve(spec, args),
        None => from_template("fair"),
    };
    log::info!(
        "weather {}: visibility {:.0} m, precipitation {:?}, {:.1} C",
        w.name,
        w.visibility_m,
        w.precip.kind,
        w.temp_c
    );
    publish(&w);
    w
}

pub fn resolve_ground_snow(args: &Args) -> bool {
    match args.weather.as_deref().filter(|s| !is_cycle(Some(s)) && !s.trim().is_empty()) {
        Some(spec) => resolve(spec, args).snowy_ground(),
        None => false,
    }
}

pub fn publish(w: &Weather) {
    crate::scene::SNOW_WEATHER.store(w.snowy_ground(), std::sync::atomic::Ordering::Relaxed);
    ::simulation::host::set_ambient_weather(w.temp_c, w.abs_humidity());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_one_off_report_differs_from_live_sync() {
        assert_eq!(metar_mode("metar-once:EGNX"), Some((MetarMode::Once, "EGNX")));
        assert_eq!(metar_mode("metar-sync:EGNX"), Some((MetarMode::Sync, "EGNX")));
        assert_eq!(metar_mode("METAR:EGNX"), Some((MetarMode::Legacy, "EGNX")));
        assert_eq!(metar_mode("template:rain"), None);
    }

    #[test]
    fn a_weather_survives_the_wire() {
        let mut w = metar::parse("EDDB 241250Z 23008KT 4000 -RA BKN012 OVC030 14/12 Q0997");
        w.source = Source::Custom;
        w.brightness = 0.6;
        w.ground_wet = 0.7;
        let back = from_wire(&wire(&w).unwrap()).unwrap();
        assert_eq!(back.precip.kind, PrecipKind::Rain);
        assert!((back.brightness - 0.6).abs() < 0.01 && (back.ground_wet - 0.7).abs() < 0.01);
        let report = metar::parse("EDDB 241250Z 23008KT 4000 -RA OVC030 14/12 Q0997 TEMPO 1000 +RA");
        let text = wire(&report).unwrap();
        assert!(text.starts_with(REPORT) && !text.contains("TEMPO"));
    }
}
