use super::model::*;

const KT: f32 = 0.514_444;
const FT: f32 = 0.3048;

pub(super) fn is_trend(t: &str) -> bool {
    matches!(t, "TEMPO" | "BECMG" | "NOSIG" | "RMK" | "FM" | "INTER") || t.starts_with("PROB") || t.starts_with("FM") && t.len() >= 6 && t[2..].bytes().all(|b| b.is_ascii_digit())
}

fn parse_wind(t: &str) -> Option<(f32, f32, f32)> {
    if !t.is_ascii() {
        return None;
    }
    let (body, unit) = if let Some(b) = t.strip_suffix("KT") {
        (b, KT)
    } else if let Some(b) = t.strip_suffix("MPS") {
        (b, 1.0)
    } else if let Some(b) = t.strip_suffix("KMH") {
        (b, 1.0 / 3.6)
    } else {
        return None;
    };
    if body.len() < 5 {
        return None;
    }
    let (dir_s, rest) = body.split_at(3);
    let dir = if dir_s == "VRB" {
        0.0
    } else {
        dir_s.parse::<f32>().ok()?
    };
    let (speed_s, gust_s) = match rest.split_once('G') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let speed = speed_s.parse::<f32>().ok()? * unit;
    let gust = match gust_s {
        Some(g) => g.parse::<f32>().ok()? * unit,
        None => 0.0,
    };
    Some((dir.rem_euclid(360.0), speed, gust))
}

fn parse_statute_miles(t: &str) -> Option<f32> {
    let s = t.strip_suffix("SM")?;
    let s = s.strip_prefix('P').or_else(|| s.strip_prefix('M')).unwrap_or(s);
    if let Some((a, b)) = s.split_once('/') {
        let (a, b) = (a.parse::<f32>().ok()?, b.parse::<f32>().ok()?);
        return (b > 0.0).then_some(a / b);
    }
    s.parse::<f32>().ok()
}

fn parse_cloud(t: &str) -> Option<Option<Layer>> {
    if matches!(t, "NSC" | "SKC" | "CLR" | "NCD") {
        return Some(None);
    }
    for (code, cover) in [("FEW", 0.19), ("SCT", 0.44), ("BKN", 0.75), ("OVC", 1.0), ("VV", 1.0)] {
        let Some(rest) = t.strip_prefix(code) else {
            continue;
        };
        if !rest.is_ascii() || rest.len() < 3 {
            return None;
        }
        let (h, tail) = rest.split_at(3);
        let base = if h == "///" {
            900.0
        } else {
            h.parse::<f32>().ok()? * 100.0 * FT
        };
        let kind = match tail {
            "" | "///" => CloudKind::Plain,
            "CB" => CloudKind::Storm,
            "TCU" => CloudKind::Towering,
            _ => return None,
        };
        return Some(Some(Layer {
            cover,
            base_m: base.max(30.0),
            kind,
        }));
    }
    None
}

fn parse_temperature(t: &str) -> Option<(f32, Option<f32>)> {
    let (a, b) = t.split_once('/')?;
    let num = |s: &str| -> Option<f32> {
        let (neg, d) = match s.strip_prefix('M') {
            Some(r) => (true, r),
            None => (false, s),
        };
        if d.len() != 2 || !d.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let v: f32 = d.parse().ok()?;
        Some(if neg { -v } else { v })
    };
    let temp = num(a)?;
    let dew = if b.is_empty() || b == "//" { None } else { Some(num(b)?) };
    Some((temp, dew))
}

#[derive(Default)]
struct Phenomenon {
    intensity: i8,
    recent: bool,
    vicinity: bool,
    shower: bool,
    thunder: bool,
    patchy: bool,
    freezing: bool,
    precip: Option<PrecipKind>,
    haze: Option<Haze>,
}

fn parse_phenomenon(t: &str) -> Option<Phenomenon> {
    if !t.is_ascii() || t.is_empty() {
        return None;
    }
    let mut p = Phenomenon::default();
    let mut s = t;
    if let Some(r) = s.strip_prefix("RE") {
        p.recent = true;
        s = r;
    }
    match s.as_bytes().first() {
        Some(b'-') => {
            p.intensity = -1;
            s = &s[1..];
        }
        Some(b'+') => {
            p.intensity = 1;
            s = &s[1..];
        }
        _ => {}
    }
    if let Some(r) = s.strip_prefix("VC") {
        p.vicinity = true;
        s = r;
    }
    if s.is_empty() {
        return None;
    }
    while !s.is_empty() {
        if s.len() < 2 {
            return None;
        }
        let (code, rest) = s.split_at(2);
        match code {
            "MI" | "BC" | "PR" => p.patchy = true,
            "DR" | "BL" => {}
            "SH" => p.shower = true,
            "TS" => p.thunder = true,
            "FZ" => p.freezing = true,
            "DZ" => p.precip = Some(PrecipKind::Drizzle),
            "RA" | "UP" => p.precip = Some(PrecipKind::Rain),
            "SN" | "SG" => p.precip = Some(PrecipKind::Snow),
            "PL" => p.precip = Some(PrecipKind::Sleet),
            "GR" | "GS" => p.precip = Some(PrecipKind::Hail),
            "IC" | "PO" | "SQ" | "FC" | "SS" | "DS" => {}
            "BR" => p.haze = Some(Haze::Mist),
            "FG" => p.haze = Some(Haze::Fog),
            "HZ" | "FU" | "VA" | "DU" | "SA" => p.haze = Some(Haze::Haze),
            _ => return None,
        }
        s = rest;
    }
    Some(p)
}

fn precip_strength(kind: PrecipKind, intensity: i8) -> f32 {
    let base = match intensity {
        -1 => 0.28,
        1 => 0.92,
        _ => 0.6,
    };
    match kind {
        PrecipKind::Drizzle => base * 0.45,
        _ => base,
    }
}

pub fn parse(text: &str) -> Weather {
    let mut w = Weather {
        description: text.trim().to_string(),
        ..Default::default()
    };
    let toks: Vec<&str> = text.split_whitespace().collect();
    let mut station = String::new();
    let mut seen_time = false;
    let mut have_vis = false;
    let mut temp: Option<f32> = None;
    let mut dew: Option<f32> = None;
    let mut strongest = 0.0f32;
    let mut patchy_fog = false;
    let mut recent_wet = false;
    let mut real_precip = false;
    let mut vicinity_seen = false;
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i].trim_end_matches('=');
        i += 1;
        if is_trend(t) {
            break;
        }
        if !t.is_ascii() || matches!(t, "METAR" | "SPECI" | "AUTO" | "COR" | "NIL" | "") {
            continue;
        }
        if t.len() == 7 && t.ends_with('Z') && t[..6].bytes().all(|b| b.is_ascii_digit()) {
            seen_time = true;
            continue;
        }
        if !seen_time && station.is_empty() && t.len() == 4 && t.bytes().all(|b| b.is_ascii_alphanumeric()) {
            station = t.to_ascii_uppercase();
            continue;
        }
        if t == "CAVOK" {
            w.visibility_m = 50_000.0;
            w.clouds.clear();
            have_vis = true;
            continue;
        }
        if let Some((dir, speed, gust)) = parse_wind(t) {
            w.wind_dir = dir;
            w.wind_speed = speed;
            w.wind_gust = gust;
            continue;
        }
        if t.len() == 7 && t.as_bytes()[3] == b'V' && t[..3].bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if t.starts_with('R') && t.contains('/') && t.len() > 5 {
            continue;
        }
        if !have_vis {
            let digits = t.bytes().take_while(|b| b.is_ascii_digit()).count();
            if digits == 4 && t[4..].bytes().all(|b| b.is_ascii_alphabetic()) && t.len() <= 7 {
                let v: f32 = t[..4].parse().unwrap_or(9999.0);
                w.visibility_m = if v >= 9999.0 { 50_000.0 } else { v.max(50.0) };
                have_vis = true;
                continue;
            }
            let whole = (t.len() <= 2 && !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
                .then(|| t.parse::<f32>().ok())
                .flatten();
            if let (Some(whole), Some(next)) = (whole, toks.get(i)) {
                if next.contains('/') {
                    if let Some(frac) = parse_statute_miles(next) {
                        w.visibility_m = ((whole + frac) * 1609.0).max(50.0);
                        have_vis = true;
                        i += 1;
                        continue;
                    }
                }
            }
            if let Some(sm) = parse_statute_miles(t) {
                w.visibility_m = if sm >= 10.0 { 50_000.0 } else { (sm * 1609.0).max(50.0) };
                have_vis = true;
                continue;
            }
        }
        if let Some(c) = parse_cloud(t) {
            if let Some(layer) = c {
                w.clouds.push(layer);
            }
            continue;
        }
        if let Some((tc, td)) = parse_temperature(t) {
            temp = Some(tc);
            dew = td;
            continue;
        }
        if let Some(q) = t.strip_prefix('Q').filter(|q| q.len() == 4).and_then(|q| q.parse::<f32>().ok()) {
            w.pressure_hpa = q;
            continue;
        }
        if let Some(a) = t.strip_prefix('A').filter(|a| a.len() == 4).and_then(|a| a.parse::<f32>().ok()) {
            w.pressure_hpa = a / 100.0 * 33.8639;
            continue;
        }
        if let Some(p) = parse_phenomenon(t) {
            if p.recent {
                recent_wet |= p.precip.is_some();
                continue;
            }
            if p.thunder {
                w.precip.thunder = true;
            }
            if p.shower {
                w.precip.showers = true;
            }
            if let Some(h) = p.haze {
                patchy_fog |= p.patchy && h == Haze::Fog;
                w.haze = match (w.haze, h) {
                    (Haze::Fog, _) | (_, Haze::Fog) => Haze::Fog,
                    (Haze::Mist, _) | (_, Haze::Mist) => Haze::Mist,
                    _ => Haze::Haze,
                };
            }
            let kind = match p.precip {
                Some(k) => Some(k),
                None if p.vicinity && (p.shower || p.thunder) => Some(PrecipKind::Rain),
                None => None,
            };
            if let Some(k) = kind {
                let own = p.precip.is_some();
                if own && !p.vicinity {
                    real_precip = true;
                } else {
                    vicinity_seen = true;
                }
                let s = precip_strength(k, if own { p.intensity } else { 0 });
                let beats = s > strongest || (s == strongest && k == PrecipKind::Rain);
                if beats && (own && !p.vicinity || !real_precip) {
                    strongest = s;
                    w.precip.kind = k;
                    w.precip.intensity = s;
                }
            }
            continue;
        }
    }
    w.precip.vicinity = vicinity_seen && !real_precip;
    if w.precip.thunder && w.precip.kind == PrecipKind::None {
        w.precip.kind = PrecipKind::Rain;
        w.precip.intensity = 0.5;
    }
    if w.precip.kind == PrecipKind::None {
        w.precip.showers = false;
        w.precip.thunder = false;
        w.precip.vicinity = false;
    }
    let tc = temp.unwrap_or(15.0);
    w.temp_c = tc;
    w.dew_c = dew.unwrap_or(tc - 6.0).min(tc);
    w.clouds.sort_by(|a, b| a.base_m.total_cmp(&b.base_m));
    if w.haze == Haze::Fog && !patchy_fog && w.visibility_m > 1000.0 {
        w.visibility_m = 800.0;
    }
    let p = w.precip;
    if p.kind != PrecipKind::None {
        if matches!(p.kind, PrecipKind::Snow | PrecipKind::Sleet) {
            w.snow_cover = tc < 2.0;
            w.snow_on_road = tc < 1.0 && p.intensity >= 0.5;
            w.ground_wet = if tc < 0.0 { 0.1 } else { 0.4 };
        } else {
            w.ground_wet = 0.45 + 0.55 * p.intensity.min(1.0);
        }
        if p.vicinity {
            w.ground_wet *= 0.3;
        }
    } else if recent_wet {
        w.ground_wet = 0.5;
    } else if w.haze == Haze::Fog {
        w.ground_wet = 0.25;
    }
    w.name = if station.is_empty() {
        "METAR".into()
    } else {
        format!("METAR {station}")
    };
    w.source = Source::Metar(station);
    w
}

fn signed(v: f32) -> String {
    let r = v.round() as i32;
    if r < 0 {
        format!("M{:02}", -r)
    } else {
        format!("{r:02}")
    }
}

fn precip_code(p: &Precip) -> Option<String> {
    let (code, light, heavy) = match p.kind {
        PrecipKind::None => {
            return if p.thunder {
                Some(if p.vicinity { "VCTS".into() } else { "TS".into() })
            } else {
                None
            }
        }
        PrecipKind::Drizzle => ("DZ", 0.22, 0.37),
        PrecipKind::Rain => ("RA", 0.4, 0.8),
        PrecipKind::Snow => ("SN", 0.4, 0.8),
        PrecipKind::Sleet => ("PL", 0.4, 0.8),
        PrecipKind::Hail => ("GR", 0.4, 0.8),
    };
    let mut s = String::new();
    if p.vicinity {
        s.push_str("VC");
    } else if p.intensity < light {
        s.push('-');
    } else if p.intensity >= heavy {
        s.push('+');
    }
    if p.thunder {
        s.push_str("TS");
    }
    if p.showers {
        s.push_str("SH");
    }
    if !p.vicinity || (!p.thunder && !p.showers) {
        s.push_str(code);
    }
    Some(s)
}

pub fn encode(w: &Weather, station: &str) -> String {
    let mut o: Vec<String> = Vec::new();
    let station = if station.len() == 4 { station.to_ascii_uppercase() } else { "ZZZZ".into() };
    o.push(station);
    o.push("010000Z".into());
    let kt = (w.wind_speed / KT).round() as i32;
    if kt < 1 {
        o.push("00000KT".into());
    } else {
        let d = ((w.wind_dir.rem_euclid(360.0) / 10.0).round() as i32 * 10).rem_euclid(360);
        let gust_kt = (w.wind_gust / KT).round() as i32;
        let g = if gust_kt >= kt + 5 { format!("G{gust_kt:02}") } else { String::new() };
        o.push(format!("{:03}{kt:02}{g}KT", if d == 0 { 360 } else { d }));
    }
    let low_cloud = w.clouds.iter().any(|l| l.base_m < 1524.0 || l.kind != CloudKind::Plain);
    let cavok = w.visibility_m >= 9999.0
        && !low_cloud
        && w.precip.kind == PrecipKind::None
        && !w.precip.thunder
        && w.haze == Haze::None;
    if cavok && w.clouds.is_empty() {
        o.push("CAVOK".into());
    } else {
        let v = w.visibility_m;
        let step = if v < 800.0 { 50.0 } else if v < 5000.0 { 100.0 } else { 1000.0 };
        o.push(if v >= 9999.0 { "9999".into() } else { format!("{:04}", ((v / step).round() * step) as i32) });
        if let Some(c) = precip_code(&w.precip) {
            o.push(c);
        }
        match w.haze {
            Haze::Fog => o.push("FG".into()),
            Haze::Mist => o.push("BR".into()),
            Haze::Haze => o.push("HZ".into()),
            Haze::None => {}
        }
        if w.clouds.is_empty() {
            o.push("NSC".into());
        }
        for l in &w.clouds {
            let kind = match l.kind {
                CloudKind::Plain => "",
                CloudKind::Towering => "TCU",
                CloudKind::Storm => "CB",
            };
            o.push(format!("{}{:03}{kind}", l.code(), ((l.base_m / FT / 100.0).round() as i32).clamp(0, 999)));
        }
    }
    o.push(format!("{}/{}", signed(w.temp_c), signed(w.dew_c.min(w.temp_c))));
    o.push(format!("Q{:04}", w.pressure_hpa.round() as i32));
    o.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_rainy_report() {
        let w = parse("EDDB 241250Z 23008KT 4000 -RA BKN012 OVC030 14/12 Q0997");
        assert_eq!(w.wind_dir, 230.0);
        assert!((w.wind_speed - 4.1).abs() < 0.1);
        assert_eq!(w.visibility_m, 4000.0);
        assert_eq!(w.precip.kind, PrecipKind::Rain);
        assert!(w.precip.intensity < 0.4 && !w.precip.showers);
        assert_eq!(w.clouds.len(), 2);
        assert_eq!(w.pressure_hpa, 997.0);
        assert_eq!(w.temp_c, 14.0);
        assert_eq!(w.source, Source::Metar("EDDB".into()));
    }

    #[test]
    fn cavok_and_the_forecast_are_not_the_weather_now() {
        let w = parse("EDDB 241250Z VRB02KT CAVOK M03/M09 Q1030 TEMPO 4000 -RA BKN009");
        assert!(w.clouds.is_empty() && w.precip.kind == PrecipKind::None);
        assert_eq!(w.temp_c, -3.0);
    }

    #[test]
    fn showers_thunder_and_snow() {
        let s = parse("KJFK 241251Z 30012G25KT 1 1/2SM +SN VV008 M02/M03 A2992");
        assert_eq!(s.precip.kind, PrecipKind::Snow);
        assert!(s.snow_cover && (s.visibility_m - 2414.0).abs() < 5.0);
        assert!(s.wind_gust > s.wind_speed);
        let t = parse("EDDM 011200Z 25010KT 9999 VCSH SCT030 BKN045CB 20/12 Q1008");
        assert!(t.precip.vicinity && t.precip.showers);
        assert!(t.clouds.iter().any(|l| l.kind == CloudKind::Storm));
        let ts = parse("EDDM 011200Z 25010KT 3000 +TSRA BKN018CB 22/18 Q1004");
        assert!(ts.precip.thunder && ts.precip.intensity > 0.8);
    }

    #[test]
    fn written_and_read_again() {
        let w = parse("EDDB 241250Z 23008G18KT 4000 -RA BKN012 OVC030 14/12 Q0997");
        let again = parse(&encode(&w, "EDDB"));
        assert_eq!(again.precip.kind, w.precip.kind);
        assert_eq!(again.clouds.len(), w.clouds.len());
        assert_eq!(again.visibility_m, 4000.0);
        assert_eq!(again.temp_c, 14.0);
        assert_eq!(again.wind_dir, 230.0);
        assert!(encode(&Weather::default(), "").contains("CAVOK"));
    }
}
