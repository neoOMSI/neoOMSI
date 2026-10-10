use crate::controllers::{self, Connected, Controllers, DeviceCfg, Func};
use crate::game_lists::{Dropdown, HEADING, KeyView, row};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static SEL: AtomicUsize = AtomicUsize::new(0);
static LIVE: Mutex<Option<(Instant, Vec<Connected>)>> = Mutex::new(None);
static IMPORT_MSG: Mutex<Option<String>> = Mutex::new(None);
static TABMAP: Mutex<Vec<usize>> = Mutex::new(Vec::new());

type Rows = Vec<(String, String)>;

const LONG_LIST: usize = 8;

fn connected(pads: Option<&Controllers>) -> Vec<Connected> {
    let Ok(mut g) = LIVE.lock() else {
        return Vec::new();
    };
    if g.as_ref().map_or(true, |(t, v)| {
        t.elapsed() > Duration::from_millis(if v.is_empty() { 200 } else { 1000 })
    }) {
        let v = pads
            .map(|c| c.connected_devices())
            .unwrap_or_default();
        *g = Some((Instant::now(), v));
    }
    g.as_ref().map(|x| x.1.clone()).unwrap_or_default()
}

fn row_import() -> (String, String) {
    let msg = IMPORT_MSG.lock().ok().and_then(|m| m.clone());
    (
        row(
            "pause.controls.import.title",
            'a',
            "pause.controls.import.name",
            msg.as_deref().unwrap_or("pause.controls.import.desc"),
            None,
        ),
        "pad_import".to_string(),
    )
}

fn set_import_msg(m: String) {
    if let Ok(mut g) = IMPORT_MSG.lock() {
        *g = Some(m);
    }
}

fn heading(text: &str) -> (String, String) {
    (row(text, 'h', "", "", None), HEADING.to_string())
}

fn tl(key: &str) -> String {
    ::i18n::translate(key, &[])
}

pub(crate) fn axis_names(gamepad: bool) -> [String; 8] {
    std::array::from_fn(|i| match (gamepad, i) {
        (true, 0..=5) => tl(&format!("pause.controls.gamepad_axis.{i}")),
        (true, _) => String::new(),
        (false, _) => tl(&format!("pause.controls.axis.name.{i}")),
    })
}

pub(crate) fn button_label(n: usize, gamepad: bool) -> String {
    const PAD: [&str; 6] = ["X", "A", "B", "Y", "LB", "RB"];
    const DIRS: [&str; 4] = ["up", "right", "down", "left"];
    let hats = controllers::HAT_BUTTONS..controllers::HAT_BUTTONS + 16;
    if cfg!(windows) && hats.contains(&n) {
        let (hat, dir) = ((n - hats.start) / 4, (n - hats.start) % 4);
        let dir = tl(&format!("pause.controls.hat.{}", DIRS[dir]));
        return if gamepad && hat == 0 {
            ::i18n::translate("pause.controls.hat.dpad", &[("dir", &dir)])
        } else {
            ::i18n::translate("pause.controls.hat.hat", &[("n", &(hat + 1)), ("dir", &dir)])
        };
    }
    if cfg!(windows) && gamepad {
        if let Some(p) = PAD.get(n) {
            return p.to_string();
        }
        if let Some(k) = ["view", "menu", "left_stick", "right_stick"].get(n.wrapping_sub(8)) {
            return tl(&format!("pause.controls.pad.{k}"));
        }
    }
    format!("{} {}", tl("pause.controls.button.name"), n + 1)
}

fn func_text(f: Option<(Func, bool)>) -> String {
    match f {
        None => tl("pause.controls.func.0"),
        Some((f, inv)) => {
            let t = tl(&format!("pause.controls.func.{}", Func::code(Some(f)) + 1));
            if inv {
                format!("{t} ({})", tl("pause.controls.reversed"))
            } else {
                t
            }
        }
    }
}

pub(crate) fn selected(devices: &[DeviceCfg]) -> Option<usize> {
    (!devices.is_empty()).then(|| SEL.load(Ordering::Relaxed).min(devices.len() - 1))
}

pub(crate) fn save(pads: Option<&mut Controllers>, devices: &[DeviceCfg]) {
    controllers::write_cfg(devices);
    let _ = ::config::save();
    if let Some(c) = pads {
        c.reload_cfg();
    }
}

fn names_of(root: &Path) -> &'static crate::describe::ControlNames {
    crate::describe::names(
        root,
        &::config::get_string("ui", "language").unwrap_or_else(|| "en".into()),
    )
}

fn actions() -> Vec<String> {
    let mut out = vec![String::new()];
    if let Ok(v) = omsi_launcher_lib::get_keybindings() {
        if let Some(a) = v.get("vehicles").and_then(|a| a.as_array()) {
            out.extend(
                a.iter()
                    .filter_map(|b| b.get("action").and_then(|x| x.as_str()).map(String::from)),
            );
        }
    }
    for a in [
        "kw_s_R_fest",
        "kw_s_1_fest",
        "kw_s_2_fest",
        "kw_s_3_fest",
        "kw_s_4_fest",
        "kw_s_5_fest",
        "kw_s_6_fest",
        "kw_s_7_fest",
        "kw_s_8_fest",
        "kw_s_9_fest",
        "kw_s_10_fest",
    ] {
        if !out.iter().any(|x| x.eq_ignore_ascii_case(a)) {
            out.push(a.to_string());
        }
    }
    for a in [
        "gear_up",
        "gear_down",
        "view_look_left",
        "view_look_right",
        "view_look_up",
        "view_look_down",
        "view_reset_direction",
        "view_interiorcam_plus",
        "view_interiorcam_minus",
        "view_toggle_viewpoint",
        "view_set_driver",
        "view_set_passenger",
        "view_set_outside",
        "sim_pause",
        "screenshot",
        "quicksave",
        "toggel_mouse_ctrl",
        "toggel_ctrler",
    ] {
        if !out.iter().any(|x| x == a) {
            out.push(a.to_string());
        }
    }
    out.dedup();
    out
}

fn shown_buttons(d: &DeviceCfg, physical: usize) -> usize {
    physical
        .max(
            d.buttons
                .iter()
                .rposition(|(a, _)| !a.trim().is_empty())
                .map_or(0, |i| i + 1),
        )
        .min(512)
}

fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let Ok(mut g) = NAMES.lock() else {
        return "";
    };
    if let Some(n) = g.iter().find(|n| **n == name) {
        return n;
    }
    let n: &'static str = Box::leak(name.to_string().into_boxed_str());
    g.push(n);
    n
}

pub(crate) fn set_tab(sub: usize) {
    let n = sub.saturating_sub(2);
    let at = TABMAP
        .lock()
        .ok()
        .and_then(|m| m.get(n).copied())
        .unwrap_or(n);
    SEL.store(at, Ordering::Relaxed);
}

pub(crate) fn rows(pads: Option<&Controllers>) -> Rows {
    let live = connected(pads);
    let devices = controllers::read_cfg();
    let mut out: Rows = Vec::new();
    let mut tab = 2;
    for c in live.iter() {
        if let Some(i) = devices
            .iter()
            .position(|d| controllers::names_match(&d.name, &c.name))
        {
            out.push((
                row(&c.name, 'o', "", "pause.controls.use.desc", None),
                format!("pad_open {i}"),
            ));
            tab += 1;
        } else {
            out.push((
                row(
                    &c.name,
                    'a',
                    "pause.controls.setup.name",
                    "pause.controls.setup.desc",
                    None,
                ),
                format!("pad_add {}", c.name),
            ));
        }
    }
    let _ = tab;
    out.push(row_import());
    out
}

pub(crate) fn device_tabs(
    pads: Option<&Controllers>,
    root: &Path,
    search: &KeyView,
) -> Vec<(&'static str, Rows)> {
    let live = connected(pads);
    let devices = controllers::read_cfg();
    let sel = selected(&devices).unwrap_or(usize::MAX);
    let mut map = Vec::new();
    let tabs = devices
        .iter()
        .enumerate()
        .filter_map(|(i, d)| {
            let dev = live
                .iter()
                .find(|c| controllers::names_match(&d.name, &c.name))?;
            map.push(i);
            let fresh = (i == sel)
                .then(|| pads.map(|c| c.connected_devices()))
                .flatten()
                .and_then(|v| {
                    v.into_iter()
                        .find(|c| controllers::names_match(&d.name, &c.name))
                });
            Some((
                intern(&d.name),
                device_rows(root, d, Some(fresh.as_ref().unwrap_or(dev)), search),
            ))
        })
        .collect();
    if let Ok(mut m) = TABMAP.lock() {
        *m = map;
    }
    tabs
}

fn device_rows(root: &Path, d: &DeviceCfg, dev: Option<&Connected>, search: &KeyView) -> Rows {
    let mut out: Rows = vec![(
        row(
            "pause.controls.use.name",
            's',
            if d.enabled { "on" } else { "off" },
            "pause.controls.use.desc",
            None,
        ),
        "pad_on".to_string(),
    )];
    let labels = axis_names(dev.is_some_and(|c| c.gamepad));
    out.push(heading("pause.controls.axes"));
    for a in 0..8 {
        if labels[a].is_empty() && d.axes[a].is_none() {
            continue;
        }
        let v = func_text(d.axes[a]);
        let name = if labels[a].is_empty() {
            "-"
        } else {
            labels[a].as_str()
        };
        let mut r = row(name, 'o', &v, "pause.controls.axis.desc", None);
        if let Some((_, x)) = dev.and_then(|c| c.axes.iter().find(|(k, _)| *k == a)) {
            let cal = d.calibrated(a, *x);
            let (inv, pedal) = match d.axes[a] {
                Some((f, inv)) => (
                    inv,
                    matches!(f, Func::Throttle | Func::Brake | Func::Clutch),
                ),
                None => (false, false),
            };
            let v = if inv { -cal } else { cal }.clamp(-1.0, 1.0);
            r = if pedal {
                format!("{r}\u{1f}\u{1f}{:.3}\u{1f}u", (v + 1.0) * 0.5)
            } else {
                format!("{r}\u{1f}\u{1f}{:.3}", v)
            };
        }
        out.push((r, format!("pad_axis {a}")));
    }
    out.push((
        row(
            "pause.controls.deadzone.name",
            'o',
            &format!(
                "{:.0} %",
                d.deadzone.unwrap_or_else(controllers::global_deadzone) * 100.0
            ),
            "pause.controls.deadzone.desc",
            None,
        ),
        "pad_dz".to_string(),
    ));

    if dev.is_some_and(|c| c.ff_capable) || d.ff_scale.is_some() || d.ff_invert.is_some() {
        let (steer, vib) = d.ff_scale.unwrap_or((1.0, 1.0));
        out.push(heading("pause.controls.ff.title"));
        out.push((
            row(
                "pause.controls.ff.steer.name",
                'o',
                &format!("{:.0} %", steer * 100.0),
                "pause.controls.ff.steer.desc",
                None,
            ),
            "pad_ffs".to_string(),
        ));
        out.push((
            row(
                "pause.controls.ff.vib.name",
                'o',
                &format!("{:.0} %", vib * 100.0),
                "pause.controls.ff.vib.desc",
                None,
            ),
            "pad_ffv".to_string(),
        ));
        if !dev.is_some_and(|c| c.gamepad) {
            let inv = d.ff_invert.unwrap_or_else(controllers::global_ff_invert);
            out.push((
                row(
                    "pause.controls.ff.invert.name",
                    's',
                    if inv { "on" } else { "off" },
                    "pause.controls.ff.invert.desc",
                    None,
                ),
                "pad_ffinv".to_string(),
            ));
        }
    }
    let n = shown_buttons(d, dev.map_or(0, |c| c.buttons));
    if n > 0 {
        let names = names_of(root);
        let gamepad = dev.is_some_and(|c| c.gamepad);
        out.push((
            row("pause.controls.buttons", 'h', &tl("pause.controls.find_hint"), "", None),
            HEADING.to_string(),
        ));
        let q = search.filter.trim().to_lowercase();
        if n > LONG_LIST {
            out.push((
                row(
                    "pause.controls.button_search",
                    if search.searching { 'F' } else { 'f' },
                    search.filter,
                    "",
                    None,
                ),
                "keysearch".to_string(),
            ));
        }
        let before = out.len();
        for b in 0..n {
            let act = d.buttons.get(b).map(|x| x.0.trim()).unwrap_or("");
            let v = if act.is_empty() {
                "-".to_string()
            } else {
                names.key_label(act)
            };
            let label = button_label(b, gamepad);
            if n > LONG_LIST
                && !q.is_empty()
                && !label.to_lowercase().contains(&q)
                && (act.is_empty() || !v.to_lowercase().contains(&q))
            {
                continue;
            }
            let mut r = row(&label, 'o', &v, "pause.controls.button.desc", None);
            if controllers::is_pressed(&d.name, b) {
                r.push_str("\u{1f}\u{1f}\u{1f}\u{1f}1");
            }
            out.push((r, format!("pad_btn {b}")));
        }
        if out.len() == before {
            let none = ::i18n::translate("pause.controls.no_match", &[("query", &search.filter.trim())]);
            out.push((row(&none, 'i', "", "", None), "noop".to_string()));
        }
    }
    out
}

pub(crate) fn dropdown(root: &Path, row_k: usize, id: &str) -> Option<Dropdown> {
    let devices = controllers::read_cfg();
    let d = &devices[selected(&devices)?];
    let (verb, arg) = id.split_once(' ').unwrap_or((id, ""));
    let mut items: Vec<(String, String)> = Vec::new();
    let mut current = None;
    let mut search: Vec<String> = Vec::new();
    match verb {
        "pad_axis" => {
            let a: usize = arg.parse().ok().filter(|a| *a < 8)?;
            let now = d.axes[a];
            items.push((func_text(None), format!("pad_set_axis {a} -1 0")));
            if now.is_none() {
                current = Some(0);
            }
            for c in 0..7 {
                for inv in [false, true] {
                    let f = Func::from_code(c);
                    if now == f.map(|f| (f, inv)) {
                        current = Some(items.len());
                    }
                    items.push((
                        func_text(f.map(|f| (f, inv))),
                        format!("pad_set_axis {a} {c} {}", inv as u8),
                    ));
                }
            }
        }
        "pad_dz" => {
            let now =
                (d.deadzone.unwrap_or_else(controllers::global_deadzone) * 100.0).round() as usize;
            for v in 0..=30usize {
                if v == now {
                    current = Some(v);
                }
                items.push((format!("{v} %"), format!("pad_set_dz {v}")));
            }
        }
        "pad_ffs" | "pad_ffv" => {
            let steer = verb == "pad_ffs";
            let (a, b) = d.ff_scale.unwrap_or((1.0, 1.0));
            let now = ((if steer { a } else { b }) * 20.0).round() as usize;
            for v in 0..=40usize {
                if v == now {
                    current = Some(v);
                }
                items.push((
                    format!("{} %", v * 5),
                    format!("pad_set_ff {} {}", if steer { "s" } else { "v" }, v * 5),
                ));
            }
        }
        "pad_btn" => {
            let b: usize = arg.parse().ok()?;
            let names = names_of(root);
            let now = d
                .buttons
                .get(b)
                .map(|x| x.0.trim().to_string())
                .unwrap_or_default();
            let bound = omsi_launcher_lib::get_keybindings().ok();
            for (i, a) in actions().into_iter().enumerate() {
                if a.eq_ignore_ascii_case(&now) {
                    current = Some(i);
                }
                let label = if a.is_empty() {
                    func_text(None)
                } else {
                    names.key_label(&a)
                };
                let mut hay = format!("{label} {a}");
                for sec in ["vehicles", "game"] {
                    for e in bound
                        .iter()
                        .filter_map(|v| v.get(sec)?.as_array())
                        .flatten()
                    {
                        let scan = e.get("scan_code").and_then(|x| x.as_i64()).unwrap_or(0);
                        if scan != 0
                            && e.get("action")
                            .and_then(|x| x.as_str())
                            .is_some_and(|x| !a.is_empty() && x.eq_ignore_ascii_case(&a))
                        {
                            let m = e.get("modifier").and_then(|x| x.as_i64()).unwrap_or(0);
                            hay.push(' ');
                            hay.push_str(&crate::keys::key_name(scan, m));
                        }
                    }
                }
                search.push(hay.to_lowercase());
                items.push((label, format!("pad_set_btn {b} {a}")));
            }
        }
        _ => return None,
    }
    let sel = current.unwrap_or(0);
    Some(Dropdown {
        row: row_k,
        all: if search.is_empty() {
            Vec::new()
        } else {
            items.clone()
        },
        items,
        sel,
        top: 0,
        current,
        search,
        filter: String::new(),
    })
}

pub(crate) fn apply(mut pads: Option<&mut Controllers>, verb: &str, arg: &str) {
    apply_inner(pads.as_deref_mut(), verb, arg);
    if let Some(c) = pads {
        c.refresh_devices();
    }
}

fn apply_inner(pads: Option<&mut Controllers>, verb: &str, arg: &str) {
    let mut devices = controllers::read_cfg();
    let Some(i) = selected(&devices) else {
        return;
    };
    if verb == "pad_set_ff" {
        let (which, v) = arg.split_once(' ').unwrap_or(("", ""));
        let Some(v) = v.parse::<f32>().ok().filter(|v| (0.0..=200.0).contains(v)) else {
            return;
        };
        let (a, b) = devices[i].ff_scale.unwrap_or((1.0, 1.0));
        devices[i].ff_scale = Some(if which == "s" {
            (v / 100.0, b)
        } else {
            (a, v / 100.0)
        });
        save(pads, &devices);
        return;
    }
    let mut it = arg.splitn(3, ' ');
    let n: Option<i64> = it.next().and_then(|x| x.parse().ok());
    match verb {
        "pad_set_axis" => {
            let (Some(a), Some(c)) = (
                n.filter(|a| (0..8).contains(a)),
                it.next().and_then(|x| x.parse::<i32>().ok()),
            ) else {
                return;
            };
            let inv = it.next() == Some("1");
            devices[i].axes[a as usize] = Func::from_code(c).map(|f| (f, inv));
        }
        "pad_set_dz" => {
            let Some(v) = n else {
                return;
            };
            devices[i].deadzone = Some(v as f32 / 100.0);
            for c in devices[i].calibration.iter_mut().flatten() {
                c.deadzone = None;
            }
        }
        "pad_set_btn" => {
            let Some(b) = n.filter(|b| (0..512).contains(b)) else {
                return;
            };
            let b = b as usize;
            let rest = arg.splitn(2, ' ').nth(1).unwrap_or("").trim().to_string();
            if devices[i].buttons.len() <= b {
                devices[i]
                    .buttons
                    .resize(b + 1, (String::new(), "0".into()));
            }
            devices[i].buttons[b].0 = rest;
        }
        _ => return,
    }
    save(pads, &devices);
}

/// The menu's sub-tab to show next, when the click opens a device's page.
pub(crate) fn click(mut pads: Option<&mut Controllers>, verb: &str, arg: &str) -> Option<usize> {
    let tab = click_inner(pads.as_deref_mut(), verb, arg);
    if let Some(c) = pads {
        c.refresh_devices();
    }
    tab
}

fn click_inner(pads: Option<&mut Controllers>, verb: &str, arg: &str) -> Option<usize> {
    if verb == "pad_import" {
        let done = omsi_launcher_lib::omsi_gamectrler_cfg()
            .map_err(|e| e.to_string())
            .and_then(|p| controllers::import_omsi_cfg(&p));
        match done {
            Ok(names) => {
                let _ = ::config::save();
                if let Some(c) = pads {
                    c.reload_cfg();
                }
                log::info!("controllers imported from OMSI 2: {}", names.join(", "));
                // TODO: Move this to notification UI (not implemented yet)
                set_import_msg(format!("Imported {}: {}", names.len(), names.join(", ")));
            }
            Err(e) => {
                log::warn!("controller import: {e}");
                // TODO: Move this to notification UI (not implemented yet)
                set_import_msg(format!("Import failed: {e}"));
            }
        }
        return None;
    }
    let mut devices = controllers::read_cfg();
    match verb {
        "pad_add" => {
            devices.push(DeviceCfg {
                name: arg.to_string(),
                second: "0".into(),
                ..Default::default()
            });
            let at = devices.len() - 1;
            SEL.store(at, Ordering::Relaxed);
            save(pads, &devices);
            return Some(TABMAP.lock().map_or(0, |m| m.len()) + 2);
        }
        "pad_open" => {
            let i = arg.parse::<usize>().ok().filter(|i| *i < devices.len())?;
            let t = TABMAP
                .lock()
                .ok()
                .and_then(|m| m.iter().position(|x| *x == i))?;
            SEL.store(i, Ordering::Relaxed);
            return Some(t + 2);
        }
        "pad_on" => {
            if let Some(i) = selected(&devices) {
                devices[i].enabled = !devices[i].enabled;
                save(pads, &devices);
            }
        }
        "pad_ffinv" => {
            if let Some(i) = selected(&devices) {
                let now = devices[i]
                    .ff_invert
                    .unwrap_or_else(controllers::global_ff_invert);
                devices[i].ff_invert = Some(!now);
                save(pads, &devices);
            }
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::button_label;

    #[cfg(windows)]
    #[test]
    fn pad_buttons_have_the_launchers_names() {
        let h = crate::controllers::HAT_BUTTONS;
        assert_eq!(button_label(1, true), "A");
        assert_eq!(button_label(4, true), "LB");
        assert_eq!(button_label(9, true), "Menu");
        assert_eq!(button_label(h, true), "D-pad up");
        assert_eq!(button_label(h + 5, false), "Hat 2 right");
        assert_eq!(button_label(6, true), "Button 7");
    }

    #[test]
    fn other_buttons_are_numbered() {
        assert_eq!(button_label(1, false), "Button 2");
        assert_eq!(button_label(20, true), "Button 21");
    }
}
