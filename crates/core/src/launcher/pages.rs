//! The launcher's other pages: the driver's profile, the settings, the key bindings, the
//! running games, the mods and where things are.

use super::state::{fmt_bytes, hhmm, short_map};
use super::theme::*;
use super::ui::{ButtonKind, Ui, id_of};
use super::Launcher;
use glam::Vec2;
use omsi_launcher_lib as core;
use ::user_interface::paint::Align;
use ::user_interface::{Color, Rect, Weight};
use serde_json::{Value, json};

#[derive(Default)]
pub struct PagesView {
    pub new_driver: String,
    pub confirm_delete: Option<std::time::Instant>,
    pub drop_hover: bool,
    pub setup_root: Option<String>,
    pub tt: super::timetable::TimetableView,
}

// --- profile --------------------------------------------------------------------------------

pub fn profile(l: &mut Launcher, area: Rect) {
    let body = l.page_title(
        area,
        "Profile",
        "The driver whose personnel file the game writes: hours, kilometres, punctuality, tickets.",
    );
    let left_w = (body.w * 0.56).min(700.0);
    let left = Rect::new(body.x, body.y, left_w, body.h);
    let right = Rect::new(
        body.x + left_w + GAP * 2.0,
        body.y,
        body.w - left_w - GAP * 2.0,
        body.h,
    );
    // driver chooser
    let top = Rect::new(left.x, left.y, left.w, 132.0);
    l.ui.panel(top);
    let inner = l.ui.heading(
        Rect::new(top.x + 18.0, top.y + 14.0, top.w - 36.0, top.h - 28.0),
        "Driver",
        Some("person"),
    );
    let names = l.state.profiles.clone();
    let mut sel = names
        .iter()
        .position(|n| *n == l.state.config.profile)
        .unwrap_or(0);
    let half = (inner.w - GAP) * 0.5;
    if !names.is_empty()
        && l.ui.select(
        "profile",
        Rect::new(inner.x, inner.y, half, ROW),
        &mut sel,
        &names,
    )
    {
        l.state.config.profile = names[sel].clone();
        let _ = core::save_config(&l.state.config);
        l.state.load_profile();
        l.state.touched();
    }
    let danger_armed = l
        .pages
        .confirm_delete
        .map(|t| t.elapsed().as_secs() < 4)
        .unwrap_or(false);
    if l.ui.button(
        "profile-delete",
        Rect::new(inner.x + half + GAP, inner.y, half, ROW),
        if danger_armed {
            "Click again to delete"
        } else {
            "Delete this driver"
        },
        Some("delete"),
        ButtonKind::Danger,
    ) {
        if danger_armed {
            let name = l.state.config.profile.clone();
            match core::delete_profile(&name) {
                Ok(()) => {
                    l.state
                        .set_status(format!("Personnel file of {name} deleted."), false);
                    l.state.load_profiles();
                }
                Err(e) => l.state.set_status(format!("{e:#}"), true),
            }
            l.pages.confirm_delete = None;
        } else {
            l.pages.confirm_delete = Some(std::time::Instant::now());
        }
    }
    let y = inner.y + ROW + 10.0;
    l.ui.text_input(
        "new-driver",
        Rect::new(inner.x, y, half, ROW),
        &mut l.pages.new_driver,
        "New driver's name",
        Some("person"),
    );
    if l.ui.button(
        "profile-create",
        Rect::new(inner.x + half + GAP, y, half, ROW),
        "Create",
        Some("add"),
        ButtonKind::Normal,
    ) {
        let name = l.pages.new_driver.trim().to_string();
        if !name.is_empty() {
            match core::create_profile(&name, "M") {
                Ok(_) => {
                    l.state.config.profile = name.clone();
                    let _ = core::save_config(&l.state.config);
                    l.pages.new_driver.clear();
                    l.state.load_profiles();
                    l.state.set_status(format!("Driver {name} created."), false);
                }
                Err(e) => l.state.set_status(format!("{e:#}"), true),
            }
        }
    }
    // level and stats
    let card = Rect::new(
        left.x,
        top.bottom() + GAP * 1.5,
        left.w,
        left.h - top.h - GAP * 1.5,
    );
    l.ui.panel(card);
    let Some(p) = l.state.profile.clone() else {
        l.ui.text_in(
            "Create a driver to start a personnel file.",
            card.pad(20.0, 20.0),
            14.0,
            Weight::Medium,
            TEXT_DIM,
            Align::Left,
        );
        return;
    };
    let c = Vec2::new(card.x + 70.0, card.y + 76.0);
    let prev = ((p.level - 1) * (p.level - 1) * 250) as f64;
    let frac =
        ((p.xp as f64 - prev) / (p.next_level_xp as f64 - prev).max(1.0)).clamp(0.0, 1.0) as f32;
    let shown = l.ui.anim(id_of("xp-ring"), frac, 0.6);
    l.ui.p().circle(c, 50.0, Color::rgba(28, 31, 37, 1.0));
    l.ui.p().arc(
        c,
        44.0,
        52.0,
        0.0,
        std::f32::consts::TAU,
        Color::WHITE.alpha(0.08),
    );
    let a0 = -std::f32::consts::FRAC_PI_2;
    l.ui.p().arc(
        c,
        44.0,
        52.0,
        a0,
        a0 + std::f32::consts::TAU * shown.max(0.01),
        ACCENT,
    );
    l.ui.text_in(
        &p.level.to_string(),
        Rect::new(c.x - 40.0, c.y - 26.0, 80.0, 40.0),
        34.0,
        Weight::Black,
        TEXT,
        Align::Center,
    );
    l.ui.text_in(
        "LEVEL",
        Rect::new(c.x - 40.0, c.y + 12.0, 80.0, 16.0),
        10.0,
        Weight::Black,
        TEXT_DIM,
        Align::Center,
    );
    let tx = card.x + 142.0;
    l.ui.text_in(
        &format!(
            "{}{}",
            p.name,
            if p.exists {
                ""
            } else {
                " (no personnel file yet)"
            }
        ),
        Rect::new(tx, card.y + 34.0, card.w - 160.0, 30.0),
        24.0,
        Weight::Black,
        TEXT,
        Align::Left,
    );
    l.ui.progress(
        Rect::new(tx, card.y + 76.0, card.w - 170.0, 10.0),
        shown,
        false,
    );
    l.ui.text_in(
        &format!(
            "{} XP · {} to level {}",
            p.xp,
            (p.next_level_xp - p.xp).max(0),
            p.level + 1
        ),
        Rect::new(tx, card.y + 94.0, card.w - 160.0, 18.0),
        12.5,
        Weight::Medium,
        TEXT_DIM,
        Align::Left,
    );
    let hours = |h: f64| {
        format!(
            "{} h {:02} min",
            h.floor() as i64,
            ((h - h.floor()) * 60.0).round() as i64
        )
    };
    let stats = [
        ("schedule", hours(p.hours), "hours driven"),
        ("route", format!("{:.1} km", p.km), "distance"),
        ("location_on", p.stops.to_string(), "stops served"),
        ("timer", format!("{} / {}", p.early, p.late), "early / late"),
        (
            "confirmation_number",
            format!("{:.0}", p.tickets),
            "tickets sold",
        ),
        ("payments", format!("{:.2}", p.cash), "takings"),
        ("warning", p.crashes.to_string(), "crashes"),
        ("person", p.hurt.to_string(), "pedestrians hurt"),
        ("speed", format!("{:.0} %", p.rating_driving), "driving"),
        (
            "airport_shuttle",
            format!("{:.0} %", p.rating_comfort),
            "comfort",
        ),
        (
            "receipt_long",
            format!("{:.0} %", p.rating_tickets),
            "ticket selling",
        ),
        ("history", p.sessions.len().to_string(), "runs"),
    ];
    let grid = Rect::new(card.x + 18.0, card.y + 150.0, card.w - 36.0, card.h - 170.0);
    let cols = 3;
    let cw = (grid.w - GAP * (cols as f32 - 1.0)) / cols as f32;
    let ch = 68.0;
    for (k, (icon, v, label)) in stats.iter().enumerate() {
        let (cx, cy) = ((k % cols) as f32, (k / cols) as f32);
        let r = Rect::new(grid.x + cx * (cw + GAP), grid.y + cy * (ch + 10.0), cw, ch);
        if r.bottom() > card.bottom() - 6.0 {
            break;
        }
        l.ui.p().rounded(r, 10.0, Color::WHITE.alpha(0.04));
        l.ui.icon(icon, Vec2::new(r.x + 22.0, r.y + 22.0), 18.0, ACCENT);
        l.ui.text_in(
            v,
            Rect::new(r.x + 40.0, r.y + 8.0, r.w - 48.0, 28.0),
            18.0,
            Weight::Black,
            TEXT,
            Align::Left,
        );
        l.ui.text_in(
            label,
            Rect::new(r.x + 14.0, r.y + 42.0, r.w - 20.0, 18.0),
            11.5,
            Weight::Medium,
            TEXT_DIM,
            Align::Left,
        );
    }
    // recent runs
    l.ui.panel(right);
    let inner = l.ui.heading(
        Rect::new(
            right.x + 18.0,
            right.y + 14.0,
            right.w - 36.0,
            right.h - 28.0,
        ),
        "Recent runs",
        Some("history"),
    );
    let sessions = p.sessions.clone();
    l.ui.scroll_area(
        "runs",
        Rect::new(inner.x - 6.0, inner.y, inner.w + 12.0, inner.h),
        &mut |ui, v| {
            if sessions.is_empty() {
                ui.text_in(
                    "No runs yet. Drive a duty and it shows up here.",
                    Rect::new(v.x + 8.0, v.y, v.w, 30.0),
                    13.0,
                    Weight::Regular,
                    TEXT_DIM,
                    Align::Left,
                );
                return 30.0;
            }
            let rh = 62.0;
            for (k, s) in sessions.iter().enumerate() {
                let r = Rect::new(v.x + 6.0, v.y + k as f32 * rh, v.w - 16.0, rh - 6.0);
                ui.p().rounded(r, 9.0, Color::WHITE.alpha(0.04));
                let title = match &s.line {
                    Some(line) => format!(
                        "Line {line}{} · {}",
                        s.tour
                            .as_ref()
                            .map(|t| format!(" / {t}"))
                            .unwrap_or_default(),
                        short_map(&s.map)
                    ),
                    None => format!("Free drive · {}", short_map(&s.map)),
                };
                ui.text_in(
                    &title,
                    Rect::new(r.x + 12.0, r.y + 6.0, r.w - 130.0, 20.0),
                    13.0,
                    Weight::Bold,
                    TEXT,
                    Align::Left,
                );
                ui.text_in(
                    &format!(
                        "{} · {:.1} km · {} stops · {} tickets · {} crashes",
                        s.bus.rsplit('/').next().unwrap_or(""),
                        s.metres / 1000.0,
                        s.stops,
                        s.tickets,
                        s.crashes
                    ),
                    Rect::new(r.x + 12.0, r.y + 28.0, r.w - 130.0, 18.0),
                    11.0,
                    Weight::Regular,
                    TEXT_DIM,
                    Align::Left,
                );
                let when = chrono_like(s.time);
                ui.text_in(
                    &when,
                    Rect::new(r.right() - 120.0, r.y + 6.0, 110.0, 20.0),
                    11.5,
                    Weight::Medium,
                    TEXT_SOFT,
                    Align::Right,
                );
                ui.text_in(
                    &hours_short(s.seconds / 3600.0),
                    Rect::new(r.right() - 120.0, r.y + 28.0, 110.0, 18.0),
                    11.5,
                    Weight::Medium,
                    ACCENT,
                    Align::Right,
                );
            }
            sessions.len() as f32 * rh
        },
    );
}

fn hours_short(h: f64) -> String {
    format!(
        "{}:{:02} h",
        h.floor() as i64,
        ((h - h.floor()) * 60.0).round() as i64
    )
}

/// A Unix time as "YYYY-MM-DD HH:MM" in the machine's time zone.
fn chrono_like(t: u64) -> String {
    #[cfg(unix)]
    {
        let tt = t as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if !unsafe { libc::localtime_r(&tt, &mut tm) }.is_null() {
            return format!(
                "{:04}-{:02}-{:02} {:02}:{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                tm.tm_hour,
                tm.tm_min
            );
        }
    }
    let days = (t / 86400) as i64;
    let secs = t % 86400;
    // civil from days (Howard Hinnant), UTC
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        secs / 3600,
        (secs % 3600) / 60
    )
}

// --- settings (updates only) -----------------------------------------------------------------

/// A value of the settings as the pages show it.
fn get<'a>(v: &'a Value, k: &str) -> &'a Value {
    v.get(k).unwrap_or(&Value::Null)
}

fn toggle_setting(ui: &mut Ui, s: &mut Value, dirty: &mut f32, r: Rect, label: &str, key: &str) {
    let mut v = get(s, key).as_bool().unwrap_or(false);
    if ui.toggle(&format!("set-{key}"), r, &mut v, label) {
        s[key] = json!(v);
        *dirty = 0.3;
    }
}

/// The settings page: only the updates from the GitHub releases (see `crate::updater`).
pub fn settings(l: &mut Launcher, area: Rect) {
    let body = l.page_title(
        area,
        "Settings",
        "Updates from the GitHub releases; every change is saved at once.",
    );
    let r = Rect::new(body.x, body.y, body.w.min(620.0), 230.0);
    l.ui.panel(r);
    let inner = l.ui.heading(
        Rect::new(r.x + 18.0, r.y + 14.0, r.w - 36.0, r.h - 28.0),
        "Updates",
        None,
    );
    let status = l.update.status();
    let mut check = false;
    let mut y = inner.y;
    let row = |y: f32| Rect::new(inner.x, y, inner.w, ROW - 2.0);
    toggle_setting(
        &mut l.ui,
        &mut l.state.settings,
        &mut l.state.settings_dirty,
        row(y),
        "Look for updates when the launcher starts",
        "update_check",
    );
    y += ROW + 4.0;
    toggle_setting(
        &mut l.ui,
        &mut l.state.settings,
        &mut l.state.settings_dirty,
        row(y),
        "Install updates without asking",
        "update_auto",
    );
    y += ROW + 4.0;
    {
        use crate::updater::Status;
        let r = row(y);
        let busy = matches!(
            status,
            Status::Checking
                | Status::Downloading { .. }
                | Status::Installing(_)
                | Status::WaitingForInstaller(_)
                | Status::Restarting(_)
        );
        if l.ui.button(
            "s-upd-check",
            Rect::new(r.x, r.y, 150.0, r.h),
            if busy { "Checking…" } else { "Check now" },
            Some("refresh"),
            ButtonKind::Normal,
        ) && !busy
        {
            check = true;
        }
        let text = match &status {
            Status::UpToDate => format!(
                "{} is the latest version",
                crate::updater::current_version()
            ),
            Status::Available(rel) => format!("{} is available", rel.version),
            Status::Failed(_) => "The last check failed".to_string(),
            _ => format!("This is neoOMSI {}", crate::updater::current_version()),
        };
        l.ui.text_in(
            &text,
            Rect::new(r.x + 162.0, r.y, r.w - 162.0, r.h),
            12.5,
            Weight::Regular,
            TEXT_DIM,
            Align::Left,
        );
    }
    y += ROW + 4.0;
    if l.ui.button(
        "s-upd-github",
        row(y),
        "github.com/neoOMSI/neoOMSI",
        Some("open_in_new"),
        ButtonKind::Ghost,
    ) {
        crate::updater::open_url(crate::updater::REPO_URL);
    }
    if check {
        l.update.check();
    }
}

// --- sessions ---------------------------------------------------------------------------------

fn ago(t: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(t);
    let s = now.saturating_sub(t);
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min", s / 60)
    } else {
        format!("{} h {} min", s / 3600, (s / 60) % 60)
    }
}

pub fn sessions(l: &mut Launcher, area: Rect) {
    let body = l.page_title(
        area,
        "Sessions",
        "The games you started, and who drives with you.",
    );
    let list = l.state.instances.clone();
    if list.is_empty() {
        let r = Rect::new(body.x, body.y, body.w.min(720.0), 120.0);
        l.ui.panel(r);
        l.ui.icon(
            "sports_esports",
            Vec2::new(r.x + 40.0, r.center().y),
            36.0,
            TEXT_FAINT,
        );
        l.ui.paragraph("No game is running. Start a duty on the Drive page; to drive with friends, turn on hosting on the Multiplayer page and give them the code shown here.", Vec2::new(r.x + 76.0, r.y + 30.0), r.w - 100.0, 13.5, Weight::Regular, TEXT_DIM);
        return;
    }
    let names: std::collections::HashMap<String, String> = l
        .state
        .vehicles
        .iter()
        .map(|v| (v.file.clone(), v.name.clone()))
        .collect();
    let short_bus = |b: &str| {
        names.get(b).cloned().unwrap_or_else(|| {
            b.rsplit('/')
                .next()
                .unwrap_or("")
                .trim_end_matches(".bus")
                .to_string()
        })
    };
    let mut y = body.y - l.ui.scroll.get(&id_of("sessions")).copied().unwrap_or(0.0);
    // (the page's whole width, as the other pages: at 900 a wide window had the cards in its
    // left half and the buttons in the middle of nowhere)
    let view = Rect::new(body.x, body.y, body.w, body.h);
    let mut actions: Vec<(u32, &str)> = Vec::new();
    let mut copy: Option<String> = None;
    l.ui.push_clip(view, 0.0);
    for i in &list {
        let lan = i.lan_status.clone().unwrap_or(Value::Null);
        let role = lan.get("role").and_then(|x| x.as_str()).unwrap_or("");
        let players: Vec<Value> = lan
            .get("players")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let chat: Vec<String> = lan
            .get("chat")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let warnings: Vec<String> = lan
            .get("warnings")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let log_open = l.state.open_logs.contains(&i.pid);
        let log_lines = l.state.logs.get(&i.pid).cloned().unwrap_or_default();
        let mut h = 96.0;
        if role == "host" {
            h += 96.0;
        } else if !role.is_empty() {
            h += 24.0;
        }
        h += if players.is_empty() {
            0.0
        } else {
            26.0 + players.len() as f32 * 20.0
        };
        h += if chat.is_empty() {
            0.0
        } else {
            26.0 + chat.len().min(6) as f32 * 18.0
        };
        h += warnings.len() as f32 * 20.0;
        if log_open {
            h += 220.0;
        }
        let r = Rect::new(view.x, y, view.w, h);
        l.ui.panel(r);
        let running = i.running;
        let c = Vec2::new(r.x + 24.0, r.y + 28.0);
        if running {
            let pulse = (l.ui.time * 3.0).sin() * 0.5 + 0.5;
            l.ui.p().circle(c, 6.0 + 3.0 * pulse, OK.alpha(0.25));
        }
        l.ui.p()
            .circle(c, 6.0, if running { OK } else { TEXT_FAINT });
        let duty = i
            .line
            .as_ref()
            .map(|ln| {
                format!(
                    " · line {ln}{}",
                    i.tour
                        .as_ref()
                        .map(|t| format!(" / {t}"))
                        .unwrap_or_default()
                )
            })
            .unwrap_or_default();
        l.ui.text_in(
            &format!("{} · {}{duty}", short_map(&i.map), short_bus(&i.bus)),
            Rect::new(r.x + 42.0, r.y + 16.0, r.w - 260.0, 24.0),
            16.0,
            Weight::Black,
            TEXT,
            Align::Left,
        );
        let status = if running {
            if l.state.stopping.contains(&i.pid) || i.stopping.is_some() {
                "stopping - saving the run…".to_string()
            } else {
                format!("running for {}", ago(i.started))
            }
        } else {
            let how = if i.exit_code == Some(0) {
                String::new()
            } else if i.killed {
                " (killed - it did not end by itself, the run is not saved)".into()
            } else {
                i.exit_code
                    .map(|c| format!(" (exit code {c})"))
                    .unwrap_or_default()
            };
            format!("ended{how}")
        };
        l.ui.text_in(
            &format!("{status} · driver {}", i.profile),
            Rect::new(r.x + 42.0, r.y + 42.0, r.w - 60.0, 18.0),
            12.0,
            Weight::Regular,
            TEXT_DIM,
            Align::Left,
        );
        l.ui.text_in(
            &i.last_line,
            Rect::new(r.x + 42.0, r.y + 62.0, r.w - 60.0, 18.0),
            11.5,
            Weight::Regular,
            TEXT_FAINT,
            Align::Left,
        );
        // buttons
        let bw = 110.0;
        if running {
            let stopping = l.state.stopping.contains(&i.pid);
            if l.ui.button(
                &format!("stop-{}", i.pid),
                Rect::new(r.right() - 18.0 - bw, r.y + 14.0, bw, 34.0),
                if stopping { "Stopping…" } else { "Stop" },
                Some("close"),
                ButtonKind::Danger,
            ) && !stopping
            {
                actions.push((i.pid, "stop"));
            }
        }
        if l.ui.button(
            &format!("log-{}", i.pid),
            Rect::new(
                r.right() - 18.0 - bw - if running { bw + 8.0 } else { 0.0 },
                r.y + 14.0,
                bw,
                34.0,
            ),
            if log_open { "Hide log" } else { "Show log" },
            Some("receipt_long"),
            ButtonKind::Normal,
        ) {
            actions.push((i.pid, "log"));
        }
        let mut yy = r.y + 86.0;
        if role == "host" {
            let code = lan
                .get("code")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let box_r = Rect::new(r.x + 20.0, yy, r.w - 40.0, 84.0);
            l.ui.p().rounded(box_r, 10.0, ACCENT.alpha(0.10));
            l.ui.p().rounded_border(box_r, 10.0, 1.0, ACCENT.alpha(0.5));
            l.ui.text_in(
                "SESSION CODE",
                Rect::new(box_r.x + 16.0, box_r.y + 8.0, 200.0, 16.0),
                10.5,
                Weight::Black,
                ACCENT,
                Align::Left,
            );
            l.ui.text_in(
                &code,
                Rect::new(box_r.x + 16.0, box_r.y + 26.0, box_r.w - 170.0, 30.0),
                20.0,
                Weight::Condensed,
                TEXT,
                Align::Left,
            );
            l.ui.text_in(
                "Your friends paste it into Multiplayer → Connect by Code.",
                Rect::new(box_r.x + 16.0, box_r.y + 58.0, box_r.w - 170.0, 18.0),
                11.5,
                Weight::Regular,
                TEXT_DIM,
                Align::Left,
            );
            if l.ui.button(
                &format!("copy-{}", i.pid),
                Rect::new(box_r.right() - 146.0, box_r.y + 24.0, 130.0, 36.0),
                "Copy code",
                Some("content_copy"),
                ButtonKind::Primary,
            ) {
                copy = Some(code.clone());
            }
            yy += 96.0;
        } else if role == "client" {
            let connected = lan
                .get("connected")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let text = if let Some(rej) = lan.get("rejected").and_then(|x| x.as_str()) {
                format!("not connected: {rej}")
            } else if connected {
                format!(
                    "connected to {}",
                    lan.get("host_name").and_then(|x| x.as_str()).unwrap_or("")
                )
            } else {
                "connecting…".to_string()
            };
            l.ui.text_in(
                &format!("Multiplayer: {text}"),
                Rect::new(r.x + 42.0, yy, r.w - 60.0, 20.0),
                12.5,
                Weight::Medium,
                if connected { OK } else { WARN },
                Align::Left,
            );
            yy += 24.0;
        }
        if !players.is_empty() {
            let generic_vehicles = lan
                .get("generic_vehicles")
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            l.ui.text_in(
                "PLAYERS",
                Rect::new(r.x + 42.0, yy, 200.0, 18.0),
                10.5,
                Weight::Black,
                TEXT_FAINT,
                Align::Left,
            );
            if generic_vehicles > 0 {
                l.ui.icon("warning", Vec2::new(r.x + 130.0, yy + 9.0), 14.0, WARN);
                l.ui.text_in(
                    &format!("{generic_vehicles} generic vehicle{}", if generic_vehicles == 1 { "" } else { "s" }),
                    Rect::new(r.x + 142.0, yy, r.w - 202.0, 18.0),
                    10.5,
                    Weight::Black,
                    WARN,
                    Align::Left,
                );
            }
            yy += 22.0;
            for p in &players {
                let s = |k: &str| p.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                let pax = p.get("passengers").and_then(|x| x.as_i64()).unwrap_or(0);
                let bus = if p
                    .get("generic_bus")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false)
                {
                    format!("{} (generic bus)", short_bus(&s("bus")))
                } else {
                    short_bus(&s("bus"))
                };
                let dest = if s("destination").is_empty() {
                    String::new()
                } else {
                    format!(" · {} → {}", s("line"), s("destination"))
                };
                l.ui.text_in(
                    &format!(
                        "{} · {}{dest}{} · {}",
                        s("name"),
                        bus,
                        if pax > 0 {
                            format!(" · {pax} passengers")
                        } else {
                            String::new()
                        },
                        s("where")
                    ),
                    Rect::new(r.x + 42.0, yy, r.w - 60.0, 18.0),
                    12.5,
                    Weight::Medium,
                    TEXT_SOFT,
                    Align::Left,
                );
                yy += 20.0;
            }
        }
        if !chat.is_empty() {
            l.ui.text_in(
                "CHAT  (V in the game to write)",
                Rect::new(r.x + 42.0, yy + 4.0, 300.0, 18.0),
                10.5,
                Weight::Black,
                TEXT_FAINT,
                Align::Left,
            );
            yy += 26.0;
            for c in chat.iter().rev().take(6).rev() {
                l.ui.text_in(
                    c,
                    Rect::new(r.x + 42.0, yy, r.w - 60.0, 18.0),
                    12.0,
                    Weight::Regular,
                    TEXT_SOFT,
                    Align::Left,
                );
                yy += 18.0;
            }
        }
        for w in &warnings {
            l.ui.icon("warning", Vec2::new(r.x + 50.0, yy + 9.0), 15.0, WARN);
            l.ui.text_in(
                w,
                Rect::new(r.x + 64.0, yy, r.w - 80.0, 18.0),
                12.0,
                Weight::Medium,
                WARN,
                Align::Left,
            );
            yy += 20.0;
        }
        if log_open {
            let lr = Rect::new(r.x + 20.0, yy + 6.0, r.w - 40.0, 204.0);
            l.ui.p().rounded(lr, 8.0, Color::rgba(6, 8, 10, 0.9));
            let text: Vec<String> = log_lines.iter().rev().take(11).rev().cloned().collect();
            for (k, line) in text.iter().enumerate() {
                l.ui.text_in(
                    line,
                    Rect::new(lr.x + 10.0, lr.y + 6.0 + k as f32 * 18.0, lr.w - 20.0, 18.0),
                    11.0,
                    Weight::Regular,
                    if line.contains("ERROR") {
                        DANGER
                    } else if line.contains("WARN") {
                        WARN
                    } else {
                        TEXT_DIM
                    },
                    Align::Left,
                );
            }
        }
        y += h + GAP;
    }
    l.ui.pop_clip();
    // scrolling the list
    let content = y + l.ui.scroll.get(&id_of("sessions")).copied().unwrap_or(0.0) - body.y;
    if l.ui.hover(view) && l.ui.input.wheel.y.abs() > 0.0 {
        let s = l.ui.scroll.entry(id_of("sessions")).or_insert(0.0);
        *s = (*s - l.ui.input.wheel.y * 40.0).clamp(0.0, (content - view.h).max(0.0));
    }
    for (pid, what) in actions {
        match what {
            "stop" => l.state.stop(pid),
            _ => {
                if !l.state.open_logs.remove(&pid) {
                    l.state.open_logs.insert(pid);
                    l.state.log_tail(pid);
                }
            }
        }
    }
    if let Some(c) = copy {
        l.ui.clipboard_out = Some(c);
        l.state.set_status("Session code copied.", false);
    }
    let _ = hhmm;
}

// --- mods ----------------------------------------------------------------------------------------

pub fn mods(l: &mut Launcher, area: Rect) {
    if !l.state.mods_asked {
        l.state.load_mods();
    }
    let body = l.page_title(area, "Mods", "A bus, a map, scenery, a whole OMSI folder - as a folder or a .zip, .7z or .rar. The original OMSI 2 folder is never written to.");
    let cols = 3;
    let cw = (body.w - GAP * 2.0 * (cols as f32 - 1.0)) / cols as f32;
    let colr = |k: usize| Rect::new(body.x + k as f32 * (cw + GAP * 2.0), body.y, cw, body.h);
    // install
    let c0 = colr(0);
    l.ui.panel(c0);
    let inner = l.ui.heading(
        Rect::new(c0.x + 18.0, c0.y + 14.0, c0.w - 36.0, c0.h - 28.0),
        "Install a mod",
        Some("download"),
    );
    let mut y = inner.y;
    let half = (inner.w - GAP) * 0.5;
    if l.ui.button(
        "mod-folder",
        Rect::new(inner.x, y, half, 40.0),
        "Choose a folder",
        Some("folder_open"),
        ButtonKind::Primary,
    ) {
        if super::mobile::mobile() {
            l.browse(super::mobile::Purpose::ModFolder, "");
        } else if let Some(p) = core::pick_mod(false) {
            l.state.install(p.to_string_lossy().to_string());
        }
    }
    if l.ui.button(
        "mod-zip",
        Rect::new(inner.x + half + GAP, y, half, 40.0),
        "Choose archive",
        Some("inventory_2"),
        ButtonKind::Normal,
    ) {
        if super::mobile::mobile() {
            l.browse(super::mobile::Purpose::ModZip, "");
        } else if let Some(p) = core::pick_mod(true) {
            l.state.install(p.to_string_lossy().to_string());
        }
    }
    y += 52.0;
    l.ui.label(Rect::new(inner.x, y, inner.w, 20.0), "Archive install mode");
    y += 22.0;
    let mut m = l.state.mod_mode;
    if l.ui.segmented(
        "mod-mode",
        Rect::new(inner.x, y, inner.w, 34.0),
        &mut m,
        &["Auto", "Unpacked", "Used in place"],
    ) {
        l.state.mod_mode = m;
    }
    y += 44.0;
    let drop = Rect::new(inner.x, y, inner.w, 110.0);
    let hot = l.pages.drop_hover;
    let t = l.ui.anim(id_of("drop"), if hot { 1.0 } else { 0.0 }, 0.1);
    l.ui.p().rounded(drop, 12.0, ACCENT.alpha(0.05 + 0.12 * t));
    // a dashed edge
    let per = 2.0 * (drop.w + drop.h);
    let n = (per / 14.0) as usize;
    for k in 0..n {
        let s = k as f32 * per / n as f32;
        let p = if s < drop.w {
            Vec2::new(drop.x + s, drop.y)
        } else if s < drop.w + drop.h {
            Vec2::new(drop.right(), drop.y + s - drop.w)
        } else if s < 2.0 * drop.w + drop.h {
            Vec2::new(drop.right() - (s - drop.w - drop.h), drop.bottom())
        } else {
            Vec2::new(drop.x, drop.bottom() - (s - 2.0 * drop.w - drop.h))
        };
        l.ui.p().circle(p, 1.3, ACCENT.alpha(0.35 + 0.5 * t));
    }
    l.ui.icon(
        "upload",
        Vec2::new(drop.center().x, drop.y + 38.0),
        30.0,
        ACCENT.alpha(0.6 + 0.4 * t),
    );
    l.ui.text_in(
        "…or drop a mod folder or .zip, .7z or .rar onto this window",
        Rect::new(drop.x, drop.y + 62.0, drop.w, 30.0),
        12.5,
        Weight::Medium,
        TEXT_SOFT,
        Align::Center,
    );
    y += 122.0;
    if !l.state.mod_path.is_empty() {
        let p = l.state.mod_path.clone();
        y += l.ui.paragraph(
            &p,
            Vec2::new(inner.x, y),
            inner.w,
            11.5,
            Weight::Regular,
            TEXT_FAINT,
        );
        match l.state.mod_info.clone() {
            Some(Ok(i)) if i.is_archive => {
                let fit = if i.fits {
                    format!("fits ({} free)", fmt_bytes(i.free_bytes))
                } else {
                    format!(
                        "does not fit: needs {}, {} free",
                        fmt_bytes(i.needed_bytes),
                        fmt_bytes(i.free_bytes)
                    )
                };
                let place = if i.in_place_ok {
                    "can be used in place".to_string()
                } else {
                    i.in_place.clone()
                };
                y += l.ui.paragraph(
                    &format!(
                        "{} archive, {} files, {} unpacked - {fit}; {place}",
                        fmt_bytes(i.archive_bytes),
                        i.files,
                        fmt_bytes(i.unpacked_bytes)
                    ),
                    Vec2::new(inner.x, y),
                    inner.w,
                    12.0,
                    Weight::Regular,
                    if i.fits { TEXT_DIM } else { WARN },
                );
            }
            Some(Err(e)) => {
                y += l.ui.paragraph(
                    &e,
                    Vec2::new(inner.x, y),
                    inner.w,
                    12.0,
                    Weight::Regular,
                    DANGER,
                );
            }
            _ => {}
        }
    }
    y += 10.0;
    if let Some(m) = l.state.mods.clone() {
        l.ui.heading(
            Rect::new(inner.x, y, inner.w, 28.0),
            "The Mods folder",
            None,
        );
        y += 30.0;
        y += l.ui.paragraph(
            &format!(
                "Anything put into {} is installed by itself once it has finished copying.",
                m.inbox
            ),
            Vec2::new(inner.x, y),
            inner.w,
            12.0,
            Weight::Regular,
            TEXT_DIM,
        );
        if !m.inbox_items.is_empty() {
            l.ui.paragraph(
                &format!("In it now: {}", m.inbox_items.join(", ")),
                Vec2::new(inner.x, y + 4.0),
                inner.w,
                12.0,
                Weight::Regular,
                TEXT_SOFT,
            );
        }
    }
    // installs
    let c1 = colr(1);
    l.ui.panel(c1);
    let inner = l.ui.heading(
        Rect::new(c1.x + 18.0, c1.y + 14.0, c1.w - 36.0, c1.h - 28.0),
        "Installs",
        Some("inventory_2"),
    );
    if l.ui.button(
        "jobs-clear",
        Rect::new(c1.right() - 18.0 - 130.0, c1.y + 12.0, 130.0, 30.0),
        "Clear finished",
        None,
        ButtonKind::Ghost,
    ) {
        core::install::clear_finished();
        l.state.poll_now();
    }
    let jobs = l.state.jobs.clone();
    let mut cancel = None;
    l.ui.scroll_area("jobs", Rect::new(inner.x - 6.0, inner.y, inner.w + 12.0, inner.h), &mut |ui, v| {
        if jobs.is_empty() {
            ui.paragraph("Nothing installed since the launcher started. Big archives are checked against the free disk space before anything is unpacked; a cancelled or failed install leaves nothing behind.", Vec2::new(v.x + 6.0, v.y), v.w - 12.0, 12.5, Weight::Regular, TEXT_DIM);
            return 60.0;
        }
        let mut y = v.y;
        for j in &jobs {
            let running = j.finished.is_none();
            let msg_h = ui.paragraph_height(&j.message, v.w - 40.0, 12.0, Weight::Regular);
            let h = 50.0 + msg_h + if running { 44.0 } else { 0.0 } + j.warnings.len() as f32 * 18.0;
            let r = Rect::new(v.x + 6.0, y, v.w - 16.0, h);
            ui.p().rounded(r, 10.0, Color::WHITE.alpha(0.04));
            ui.text_in(&j.name, Rect::new(r.x + 12.0, r.y + 8.0, r.w - 120.0, 20.0), 13.5, Weight::Bold, TEXT, Align::Left);
            let sc = match j.state.as_str() {
                "done" => OK,
                "failed" => DANGER,
                "cancelled" => TEXT_DIM,
                _ => ACCENT,
            };
            ui.badge(Vec2::new(r.right() - 90.0, r.y + 10.0), &j.state.to_uppercase(), sc);
            let mut yy = r.y + 34.0;
            if running {
                let frac = if j.bytes_total > 0 { j.bytes_done as f32 / j.bytes_total as f32 } else if j.files_total > 0 { j.files_done as f32 / j.files_total as f32 } else { 0.0 };
                ui.progress(Rect::new(r.x + 12.0, yy, r.w - 24.0, 8.0), frac, true);
                ui.text_in(&format!("{} / {} files · {} / {}", j.files_done, j.files_total, fmt_bytes(j.bytes_done), fmt_bytes(j.bytes_total)), Rect::new(r.x + 12.0, yy + 10.0, r.w - 24.0, 16.0), 11.0, Weight::Regular, TEXT_DIM, Align::Left);
                yy += 30.0;
            }
            yy += ui.paragraph(&j.message, Vec2::new(r.x + 12.0, yy), r.w - 24.0, 12.0, Weight::Regular, if j.state == "failed" { DANGER } else { TEXT_SOFT });
            for w in &j.warnings {
                ui.text_in(&format!("⚠ {w}"), Rect::new(r.x + 12.0, yy, r.w - 24.0, 16.0), 11.0, Weight::Regular, WARN, Align::Left);
                yy += 18.0;
            }
            if running && ui.button(&format!("cancel-{}", j.id), Rect::new(r.x + 12.0, r.bottom() - 34.0, 100.0, 28.0), "Cancel", None, ButtonKind::Danger) {
                cancel = Some(j.id);
            }
            y += h + 10.0;
        }
        y - v.y
    });
    if let Some(id) = cancel {
        core::install::cancel(id);
        l.state.poll_now();
    }
    // content folder
    let c2 = colr(2);
    l.ui.panel(c2);
    let inner = l.ui.heading(
        Rect::new(c2.x + 18.0, c2.y + 14.0, c2.w - 36.0, c2.h - 28.0),
        "Content folder",
        Some("folder_open"),
    );
    let Some(m) = l.state.mods.clone() else {
        l.ui.text_in(
            "Reading…",
            Rect::new(inner.x, inner.y, inner.w, 20.0),
            12.5,
            Weight::Regular,
            TEXT_DIM,
            Align::Left,
        );
        return;
    };
    let mut y = inner.y;
    y += l.ui.paragraph(
        &m.content_dir,
        Vec2::new(inner.x, y),
        inner.w,
        12.0,
        Weight::Medium,
        TEXT_SOFT,
    );
    y += 6.0;
    l.ui.text_in(
        &format!("{} free on this disk", fmt_bytes(m.free_bytes)),
        Rect::new(inner.x, y, inner.w, 20.0),
        13.0,
        Weight::Bold,
        ACCENT,
        Align::Left,
    );
    y += 30.0;
    for (f, n) in &m.folders {
        l.ui.icon(
            "folder_open",
            Vec2::new(inner.x + 9.0, y + 10.0),
            16.0,
            TEXT_DIM,
        );
        l.ui.text_in(
            f,
            Rect::new(inner.x + 26.0, y, inner.w * 0.6, 20.0),
            12.5,
            Weight::Medium,
            TEXT,
            Align::Left,
        );
        l.ui.text_in(
            &format!("{n} {}", if *n == 1 { "entry" } else { "entries" }),
            Rect::new(inner.x, y, inner.w, 20.0),
            12.0,
            Weight::Regular,
            TEXT_DIM,
            Align::Right,
        );
        y += 24.0;
    }
    if !m.archives.is_empty() {
        y += 8.0;
        l.ui.heading(
            Rect::new(inner.x, y, inner.w, 28.0),
            "Archives used in place",
            None,
        );
        y += 30.0;
        for (n, b) in &m.archives {
            l.ui.text_in(
                &format!("{n}  ({})", fmt_bytes(*b)),
                Rect::new(inner.x, y, inner.w, 20.0),
                12.0,
                Weight::Regular,
                TEXT_SOFT,
                Align::Left,
            );
            y += 22.0;
        }
    }
    if !m.waiting.is_empty() {
        y += 8.0;
        l.ui.heading(
            Rect::new(inner.x, y, inner.w, 28.0),
            "Waiting for their bus",
            None,
        );
        y += 30.0;
        for w in &m.waiting {
            l.ui.text_in(
                w,
                Rect::new(inner.x, y, inner.w, 20.0),
                12.0,
                Weight::Regular,
                TEXT_DIM,
                Align::Left,
            );
            y += 22.0;
        }
    }
}

// --- tutorials --------------------------------------------------------------------------------

/// OMSI's four tutorials: what each teaches, and a button to start it.
pub fn tutorials(l: &mut Launcher, area: Rect) {
    let body = l.page_title(area, "Tutorials", "OMSI 2's own lessons: each opens its situation, with its pages beside the picture (Enter turns the page).");
    static LIST: std::sync::OnceLock<Vec<(usize, String, String)>> = std::sync::OnceLock::new();
    let list = LIST.get_or_init(omsi_launcher_lib::tutorials);
    if list.is_empty() {
        l.ui.paragraph(
            "No tutorials were found in the OMSI 2 folder (Tutorials).",
            Vec2::new(body.x, body.y),
            body.w,
            13.0,
            Weight::Regular,
            TEXT_DIM,
        );
        return;
    }
    let cols = 2;
    let cw = (body.w - GAP * 2.0) / cols as f32;
    let ch = ((body.h - GAP * 2.0) / 2.0).min(330.0);
    let mut start = None;
    for (k, (n, title, text)) in list.iter().enumerate() {
        let r = Rect::new(
            body.x + (k % cols) as f32 * (cw + GAP * 2.0),
            body.y + (k / cols) as f32 * (ch + GAP * 2.0),
            cw,
            ch,
        );
        l.ui.panel(r);
        l.ui.text_in(
            title,
            Rect::new(r.x + 18.0, r.y + 14.0, r.w - 36.0, 26.0),
            17.0,
            Weight::Bold,
            TEXT,
            Align::Left,
        );
        l.ui.push_clip(
            Rect::new(r.x + 18.0, r.y + 46.0, r.w - 36.0, r.h - 110.0),
            0.0,
        );
        l.ui.paragraph(
            text,
            Vec2::new(r.x + 18.0, r.y + 46.0),
            r.w - 36.0,
            12.5,
            Weight::Regular,
            TEXT_DIM,
        );
        l.ui.pop_clip();
        if l.ui.button(
            &format!("tut-{n}"),
            Rect::new(r.x + 18.0, r.bottom() - 58.0, 200.0, 40.0),
            "Start the lesson",
            Some("play_arrow"),
            ButtonKind::Primary,
        ) {
            start = Some(*n);
        }
    }
    if let Some(n) = start {
        l.state.launch_tutorial(n);
    }
}
