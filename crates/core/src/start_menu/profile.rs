use crate::ui::{DriveRow, ProfileView};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const REFRESH: Duration = Duration::from_secs(2);

static NAME: Mutex<Option<(Instant, ProfileView)>> = Mutex::new(None);
static FULL: Mutex<Option<(Instant, String, ProfileView)>> = Mutex::new(None);

pub(super) fn invalidate() {
    *NAME.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *FULL.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn ago(at: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let d = now.saturating_sub(at);
    if d < 3600 {
        ::i18n::translate("start.profile.ago_m", &[("n", &(d / 60))])
    } else if d < 86400 {
        ::i18n::translate("start.profile.ago_h", &[("n", &(d / 3600))])
    } else {
        ::i18n::translate("start.profile.ago_d", &[("n", &(d / 86400))])
    }
}

fn short(path: &str) -> String {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let last = parts.last().copied().unwrap_or("");
    let pick = if last.eq_ignore_ascii_case("global.cfg") && parts.len() > 1 { parts[parts.len() - 2] } else { last };
    pick.rsplit_once('.').map_or(pick, |(a, _)| a).to_string()
}

fn drive(s: &omsi_launcher_lib::Session) -> DriveRow {
    let title = match (&s.line, &s.tour) {
        (Some(l), Some(t)) => format!("{l} · {t}"),
        (Some(l), None) => l.clone(),
        _ => short(&s.map),
    };
    DriveRow { title, sub: short(&s.bus), secs: s.seconds as i64, meters: s.metres as i64, ago: ago(s.time) }
}

fn current_name() -> String {
    Some(omsi_launcher_lib::load_config().profile.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| ::i18n::translate("start.profile.default_name", &[]))
}

fn build(name: String) -> ProfileView {
    let mut v = ProfileView { name: name.clone(), ..Default::default() };
    let Ok(p) = omsi_launcher_lib::get_profile(&name) else {
        v.level = 1;
        return v;
    };
    let from = (p.level - 1).pow(2) * 250;
    v.level = p.level.max(1) as u32;
    v.xp_to_next = (p.next_level_xp - p.xp).max(0) as u32;
    v.xp_frac = ((p.xp - from) as f32 / (p.next_level_xp - from).max(1) as f32).clamp(0.0, 1.0);
    v.secs = (p.hours * 3600.0) as i64;
    v.meters = (p.km * 1000.0) as i64;
    v.stops = p.stops as i64;
    v.on_time = (p.stops > 0).then(|| (((p.stops - p.early - p.late).max(0) as f32 / p.stops as f32) * 100.0).round() as u32);
    if p.exists || !p.sessions.is_empty() {
        let r = |x: f64| Some(x.round().clamp(0.0, 100.0) as u32);
        v.ratings = [r(p.rating_driving), r(p.rating_comfort), r(p.rating_tickets)];
    }
    v.drives = p.sessions.iter().take(8).map(drive).collect();
    v
}

pub(super) fn load(full: bool) -> ProfileView {
    if !full {
        let mut c = NAME.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((t, v)) = c.as_ref().filter(|(t, _)| t.elapsed() < REFRESH) {
            let _ = t;
            return v.clone();
        }
        let v = ProfileView { name: current_name(), ..Default::default() };
        *c = Some((Instant::now(), v.clone()));
        return v;
    }
    let name = current_name();
    let mut c = FULL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, n, v)) = c.as_ref().filter(|(t, n, _)| t.elapsed() < REFRESH && *n == name) {
        let _ = n;
        return v.clone();
    }
    let v = build(name.clone());
    *c = Some((Instant::now(), name, v.clone()));
    v
}
