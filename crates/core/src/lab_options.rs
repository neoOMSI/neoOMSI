use crate::game_lists::*;
use crate::ui::{Fmt, OptGroup, OptKind, OptRow, OptShow, OPTION_GROUPS};
use crate::App;
use crate::controllers::Controllers;

pub(crate) struct Host<'a> {
    pub app: Option<&'a App>,
    pub root: &'a std::path::Path,
    pub pads: Option<&'a Controllers>,
    pub keys: KeyView<'a>,
}

impl<'a> Host<'a> {
    pub(crate) fn game(app: &'a App) -> Host<'a> {
        Host {
            app: Some(app),
            root: &app.args.root,
            pads: app.controllers.as_ref(),
            keys: KeyView::of(app),
        }
    }
}

fn shown(host: &Host, show: OptShow) -> bool {
    match show {
        OptShow::Always => true,
        OptShow::Windows => cfg!(windows),
        OptShow::VrOn => cfg!(windows) && ::config::get_bool("vr", "enabled").unwrap_or(false),
        OptShow::VrBus => host.app.is_some_and(|a| a.vr_active() && a.player.is_some()),
        OptShow::Profiles => !::config::get_subs("graphics_profiles").is_empty(),
    }
}

fn row_of(host: &Host, file: &std::sync::Arc<serde_json::Value>, r: &OptRow) -> Option<(String, String)> {
    match r.kind {
        OptKind::Switch => switch_row(host.app, r.id, r.name, r.desc),
        OptKind::Slider(f) => slider_row(host.app, r.id, r.name, r.desc, &|v| f.apply(v)),
        OptKind::Select => select_row(file, r.id, r.name, r.desc),
        OptKind::Preset => preset_row(r.name, r.desc),
        OptKind::Opens => Some(opens(r.name, r.desc, r.id)),
        OptKind::Button(text) => Some(button(r.name, text, r.desc, r.id)),
        OptKind::Mic => Some(mic_row(host.app, r)),
        OptKind::Keybinds | OptKind::Pads | OptKind::VoicePlayers => None,
    }
}

fn mic_row(app: Option<&App>, r: &OptRow) -> (String, String) {
    let mic = ::config::get_string("voice", "mic")
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "pause.options.voice.mic.default".into());
    let level = app.and_then(|a| a.voice.level());
    let desc = match (app.and_then(|a| a.voice.error.as_deref()), level) {
        (Some(e), _) => e,
        (None, None) if app.is_some() => "pause.options.voice.mic.off",
        _ => r.desc,
    };
    let mut text = row(r.name, 'o', &mic, desc, None);
    if let Some(level) = level {
        let lit = if app.is_some_and(|a| a.voice.sending().is_some()) { "1" } else { "" };
        text = format!("{text}\u{1f}\u{1f}{level:.3}\u{1f}u\u{1f}{lit}");
    }
    (text, r.id.to_string())
}

fn voice_player_rows(app: Option<&App>, r: &OptRow) -> Vec<(String, String)> {
    let Some(lan) = app.and_then(|a| a.lan.as_ref()) else {
        return Vec::new();
    };
    let mut peers: Vec<(String, u32)> = lan
        .peers()
        .filter(|p| p.pose.id != lan.my_id)
        .map(|p| (p.pose.name.clone(), p.pose.id))
        .collect();
    peers.sort();
    let muted = tk("pause.options.voice.pvol.muted", "muted");
    peers
        .into_iter()
        .filter_map(|(name, id)| {
            let name = if name.is_empty() {
                ::i18n::translate("pause.admin.player", &[("id", &id)])
            } else {
                name
            };
            slider_row(app, &format!("{} {id}", r.id), &name, r.desc, &|v| {
                if v <= 0.0 { muted.clone() } else { Fmt::Pct.apply(v) }
            })
        })
        .collect()
}

fn tk(key: &str, fallback: &str) -> String {
    let t = ::i18n::translate(key, &[]);
    if t == key { fallback.to_string() } else { t }
}

fn slug(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            o.push(c.to_ascii_lowercase());
        } else if !o.ends_with('_') {
            o.push('_');
        }
    }
    o.trim_matches('_').to_string()
}

fn key_text(v: &str) -> String {
    match v {
        "Not set" => return tk("pause.keybinds.not_set", v),
        "press a key..." => return tk("pause.keybinds.press", v),
        _ => {}
    }
    let mut rest = v;
    let mut out: Vec<String> = Vec::new();
    loop {
        let mut next = None;
        for m in ["Shift", "Ctrl", "Alt"] {
            if let Some(r) = rest.strip_prefix(m).and_then(|r| r.strip_prefix('+')) {
                out.push(tk(&format!("pause.keys.{}", slug(m)), m));
                next = Some(r);
                break;
            }
        }
        match next {
            Some(r) => rest = r,
            None => break,
        }
    }
    out.push(tk(&format!("pause.keys.{}", slug(rest)), rest));
    out.join("+")
}

fn retag((text, id): (String, String)) -> (String, String) {
    let mut p: Vec<String> = text.split('\u{1f}').map(String::from).collect();
    if p.len() < 5 {
        return (text, id);
    }
    if id == HEADING {
        p[1] = "h".into();
        p[0] = tk(&format!("pause.keybinds.group.{}", slug(&p[0])), &p[0]);
        if p[3] == "Try another name or key" {
            p[3] = tk("pause.keybinds.nothing_desc", &p[3]);
        }
    } else if id == "keysearch" {
        p[1] = if p[1] == "E" { "F" } else { "f" }.into();
        p[0] = tk("pause.keybinds.find", &p[0]);
    } else if id == "noop" && p[0] == "The key bindings could not be read" {
        p[0] = tk("pause.keybinds.error", &p[0]);
    } else if let Some(rest) = id.strip_prefix("keybind ") {
        p.push(rest.splitn(3, ' ').nth(2).unwrap_or("").to_string());
        p[2] = key_text(&p[2]);
        if p[3] == "Escape leaves it as it is" {
            p[3] = tk("pause.keybinds.cancel_desc", &p[3]);
        }
        if let Some(names) = p[3].strip_prefix("Conflict: this key is also used by ") {
            p[3] = format!("{} {names}", ::user_interface::tr("pause.keybinds.conflict"));
            p[1] = "c".into();
        }
    } else {
        return (text, id);
    }
    (p.join("\u{1f}"), id)
}

fn rows_of(host: &Host, file: &std::sync::Arc<serde_json::Value>, rows: &[OptRow]) -> Vec<(String, String)> {
    rows.iter()
        .filter(|r| shown(host, r.show))
        .flat_map(|r| match r.kind {
            OptKind::Keybinds => key_rows(host.root, &host.keys).into_iter().map(retag).collect(),
            OptKind::Pads => crate::lab_pads::rows(host.pads),
            OptKind::VoicePlayers => voice_player_rows(host.app, r),
            _ if r.id == "pax_models" => row_of(host, file, r).into_iter().chain([pax_pack_row()]).collect(),
            _ => row_of(host, file, r).into_iter().collect(),
        })
        .collect()
}

pub(crate) type Subs = Vec<(String, Vec<(String, String)>)>;

fn subs_of(host: &Host, file: &std::sync::Arc<serde_json::Value>, g: &OptGroup) -> Subs {
    let mut subs: Subs = g
        .subs
        .iter()
        .map(|s| (s.title.to_string(), rows_of(host, file, s.rows)))
        .filter(|s| !s.1.is_empty())
        .collect();

    if g.rows.iter().any(|r| r.kind == OptKind::Pads) {
        subs.extend(
            crate::lab_pads::device_tabs(host.pads, host.root, &host.keys)
                .into_iter()
                .map(|(t, r)| (t.to_string(), r)),
        );
    }
    subs
}

pub(crate) fn options_groups(host: &Host) -> Vec<(String, Vec<(String, String)>, Subs)> {
    let file = settings_file();
    OPTION_GROUPS
        .iter()
        .map(|g| (g.title.to_string(), rows_of(host, &file, g.rows), subs_of(host, &file, g)))
        .filter(|g| !g.1.is_empty() || !g.2.is_empty())
        .collect()
}