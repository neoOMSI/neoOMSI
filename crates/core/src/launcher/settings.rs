use super::Launcher;
use super::theme::*;
use super::ui::{ButtonKind, Ui};
use crate::game_lists::{self as gl, KeyView, Move};
use crate::lab_options::{Host, options_groups};
use ::user_interface::paint::Align;
use ::user_interface::{Rect, Weight};
use glam::Vec2;
use omsi_launcher_lib as core;
use std::path::PathBuf;
use std::time::{Duration, Instant};

type Rows = Vec<(String, String)>;

struct Tab {
    title: String,
    rows: Rows,
}

struct Group {
    key: String,
    title: String,
    tabs: Vec<Tab>,
}

const CONTROLS: &str = "pause.options.group.controls";
const GRAPHICS: &str = "pause.options.group.graphics";
const GAMEPLAY: &str = "pause.options.group.gameplay";
const SETUP: &str = "setup";
const UPDATES: &str = "updates";
/// The rows change behind the page (a running game, a download, a controller): read again.
const REBUILD: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct SettingsView {
    group: usize,
    tab: usize,
    groups: Vec<Group>,
    built: Option<Instant>,
    key_filter: String,
    /// The binding waiting for its key: section, entry, action.
    capture: Option<(usize, usize, String)>,
    pads: Option<crate::controllers::Controllers>,
    reset_armed: Option<Instant>,
    profile_name: String,
    pax_was: Option<crate::pax_pack::Status>,
    pub(super) wizard: Option<super::pad_wizard::Wizard>,
}

impl SettingsView {
    /// The page is left: the controllers are given back.
    pub fn leave(&mut self) {
        self.pads = None;
        self.wizard = None;
        self.capture = None;
    }

    fn group_key(&self) -> &str {
        self.groups.get(self.group).map_or("", |g| g.key.as_str())
    }
}

enum Act {
    Toggle(String),
    Slide(String, f32),
    Pick(String),
    Press(String),
    Capture(String),
    Clear(String),
}

fn tr(key: &str) -> String {
    ::i18n::translate(key, &[])
}

fn root_of(l: &Launcher) -> PathBuf {
    PathBuf::from(&l.state.config.root)
}

pub fn page(l: &mut Launcher, area: Rect) {
    let body = l.page_title(
        area,
        "Settings",
        "The game's options, the OMSI 2 folder and updates; every change is saved at once.",
    );
    keep_pads(l);
    watch_pax(l);
    take_key(l);
    if l.settings.built.is_none_or(|t| t.elapsed() > REBUILD) {
        l.settings.groups = build(l);
        l.settings.built = Some(Instant::now());
    }
    let v = &mut l.settings;
    v.group = v.group.min(v.groups.len().saturating_sub(1));
    let narrow = body.w < 760.0;
    let (list, main) = if narrow {
        (
            Rect::new(body.x, body.y, body.w, ROW),
            Rect::new(body.x, body.y + ROW + GAP, body.w, body.h - ROW - GAP),
        )
    } else {
        let w = 210.0;
        (
            Rect::new(body.x, body.y, w, body.h),
            Rect::new(body.x + w + GAP, body.y, body.w - w - GAP, body.h),
        )
    };
    let before = v.group;
    if narrow {
        let titles: Vec<String> = v.groups.iter().map(|g| g.title.clone()).collect();
        l.ui.select("set-group", list, &mut v.group, &titles);
    } else {
        l.ui.panel(list);
        let mut y = list.y + 8.0;
        for (i, g) in v.groups.iter().enumerate() {
            let r = Rect::new(list.x + 6.0, y, list.w - 12.0, ROW);
            if l.ui.row(&format!("set-g-{i}"), r, i == v.group) {
                v.group = i;
            }
            l.ui.text_in(
                &g.title,
                r.pad(14.0, 0.0),
                13.0,
                if i == v.group {
                    Weight::Medium
                } else {
                    Weight::Regular
                },
                if i == v.group { TEXT } else { TEXT_SOFT },
                Align::Left,
            );
            y += ROW + 2.0;
        }
    }
    if v.group != before {
        v.tab = 0;
        v.wizard = None;
        v.capture = None;
    }
    let Some(g) = v.groups.get(v.group) else {
        return;
    };
    l.ui.panel(main);
    let mut inner = main.pad(18.0, 14.0);
    let tab_titles: Vec<&str> = g.tabs.iter().map(|t| t.title.as_str()).collect();
    v.tab = v.tab.min(tab_titles.len().saturating_sub(1));
    if tab_titles.len() > 1 {
        let r = Rect::new(
            inner.x,
            inner.y,
            inner.w.min(150.0 * tab_titles.len() as f32),
            32.0,
        );
        let before = v.tab;
        l.ui.segmented("set-tabs", r, &mut v.tab, &tab_titles);
        if v.tab != before {
            v.wizard = None;
            crate::lab_pads::set_tab(v.tab);
        }
        inner = Rect::new(inner.x, inner.y + 44.0, inner.w, inner.h - 44.0);
    }
    let rows = match v.wizard.as_mut() {
        Some(w) => w.rows(v.pads.as_mut()),
        None => g
            .tabs
            .get(v.tab)
            .map(|t| t.rows.clone())
            .unwrap_or_default(),
    };
    let scroll = format!("set-rows-{}-{}", v.group, v.tab);
    let root = PathBuf::from(&l.state.config.root);
    if let Some(a) = draw_rows(&mut l.ui, v, &root, inner, &rows, &scroll) {
        act(l, a);
    }
}

/// The devices are opened while the controllers' group shows, and polled for the live axes.
fn keep_pads(l: &mut Launcher) {
    if l.settings.group_key() != CONTROLS {
        l.settings.pads = None;
        l.settings.wizard = None;
        return;
    }
    if l.settings.pads.is_none() {
        let hwnd = l
            .window
            .as_deref()
            .and_then(crate::controllers::window_handle);
        l.settings.pads = Some(crate::controllers::Controllers::new(&root_of(l), hwnd));
    }
    if let Some(p) = l.settings.pads.as_mut() {
        p.poll();
    }
}

/// The pack done while games run: they still have the old passengers until restarted.
fn watch_pax(l: &mut Launcher) {
    use crate::pax_pack::Status;
    let now = crate::pax_pack::status(crate::startup::content_dir);
    let busy =
        |s: &Option<Status>| matches!(s, Some(Status::Downloading { .. } | Status::Installing));
    if busy(&l.settings.pax_was) && now == Status::Installed {
        l.state.pax_changed = Some(core::install::now_secs());
    }
    l.settings.pax_was = Some(now);
}

/// A key pressed while a binding waits for it (Escape leaves it as it is).
fn take_key(l: &mut Launcher) {
    use winit::keyboard::KeyCode as K;
    let Some(code) = l.ui.input.raw_key else {
        return;
    };
    let Some((sec, idx, name)) = l.settings.capture.clone() else {
        return;
    };
    if matches!(
        code,
        K::ShiftLeft
            | K::ShiftRight
            | K::ControlLeft
            | K::ControlRight
            | K::AltLeft
            | K::AltRight
            | K::SuperLeft
            | K::SuperRight
    ) {
        return;
    }
    l.ui.input.raw_key = None;
    l.ui.input.keys.clear();
    l.settings.capture = None;
    if code == K::Escape {
        return;
    }
    let Some(scan) = crate::keys::dik_code(code) else {
        l.state.set_status(
            format!("{code:?} has no key code the game understands"),
            true,
        );
        return;
    };
    let m = ::content::input::chord(
        l.modifiers.shift_key(),
        l.modifiers.control_key(),
        l.modifiers.alt_key(),
    ) as i64;
    edit_key(
        l,
        sec,
        idx,
        &name,
        crate::game_menu::KeyEdit::Set(scan as i64, m),
    );
}

fn edit_key(l: &mut Launcher, sec: usize, idx: usize, name: &str, edit: crate::game_menu::KeyEdit) {
    let section = ["vehicles", "game"][sec.min(1)];
    let saved = core::get_keybindings().and_then(|mut v| {
        if !crate::game_menu::edit_binding(&mut v, section, idx, name, edit) {
            anyhow::bail!("the key bindings changed meanwhile: try again");
        }
        core::save_keybindings(&v)
    });
    match saved {
        Ok(()) => crate::keys::keybindings_changed(),
        Err(e) => l.state.set_status(format!("{e:#}"), true),
    }
    l.settings.built = None;
}

fn build(l: &Launcher) -> Vec<Group> {
    let root = root_of(l);
    let v = &l.settings;
    let host = Host {
        app: None,
        root: &root,
        pads: v.pads.as_ref(),
        keys: KeyView {
            filter: &v.key_filter,
            searching: false,
            capture: v.capture.as_ref().map(|c| (c.0, c.1)),
            scripted: Vec::new(),
        },
    };
    let mut groups: Vec<Group> = options_groups(&host)
        .into_iter()
        .map(|(key, rows, subs)| {
            let tab = crate::ui::OPTION_GROUPS
                .iter()
                .find(|g| g.title == key)
                .map(|g| g.tab)
                .filter(|t| !t.is_empty())
                .unwrap_or(&key);
            let mut tabs = vec![Tab {
                title: tr(tab),
                rows,
            }];
            tabs.extend(subs.into_iter().map(|(t, rows)| Tab {
                title: tr(&t),
                rows,
            }));
            Group {
                title: tr(&key),
                key,
                tabs,
            }
        })
        .collect();
    for g in groups.iter_mut() {
        let main = &mut g.tabs[0].rows;
        match g.key.as_str() {
            GRAPHICS => main.extend(profile_rows(v)),
            GAMEPLAY => {
                if let Some(at) = main.iter().position(|r| r.1 == "pax_pack_get") {
                    main.splice(at + 1..at + 1, restart_rows(l));
                }
            }
            CONTROLS => {
                for t in g.tabs.iter_mut().skip(2) {
                    t.rows.splice(0..0, super::pad_wizard::entry_rows());
                }
            }
            _ => {}
        }
    }
    groups.push(Group {
        key: SETUP.into(),
        title: "OMSI 2 folder".into(),
        tabs: vec![Tab {
            title: String::new(),
            rows: setup_rows(l),
        }],
    });
    groups.push(Group {
        key: UPDATES.into(),
        title: "Updates".into(),
        tabs: vec![Tab {
            title: String::new(),
            rows: update_rows(l),
        }],
    });
    groups
}

fn profile_rows(v: &SettingsView) -> Rows {
    let mut out = vec![
        (
            gl::row("Graphics profiles", 'h', "", "", None),
            gl::HEADING.to_string(),
        ),
        (
            gl::row(
                "Save the graphics as a profile",
                'f',
                &v.profile_name,
                "The settings of this page under a name, to load again later (Graphics profile above)",
                None,
            ),
            "gfxprofile_name".to_string(),
        ),
        (
            gl::row("", 'a', "Save profile", "", None),
            "gfxprofile_save".to_string(),
        ),
    ];
    if !::config::get_subs("graphics_profiles").is_empty() {
        out.push((
            gl::row("Delete a profile", 'o', "", "", None),
            "gfxprofile_del".to_string(),
        ));
    }
    out
}

fn restart_rows(l: &Launcher) -> Rows {
    let n = l.state.old_passenger_games();
    if n == 0 {
        return Vec::new();
    }
    vec![(
        gl::row(
            &if n == 1 {
                "A running game still has the old passengers".to_string()
            } else {
                format!("{n} running games still have the old passengers")
            },
            'a',
            "Restart",
            "They are stopped and started again where they were",
            None,
        ),
        "pax_restart".to_string(),
    )]
}

fn setup_rows(l: &Launcher) -> Rows {
    let c = &l.state.config;
    let shown = |p: &str, none: &str| {
        if p.trim().is_empty() {
            none.to_string()
        } else {
            p.to_string()
        }
    };
    let mut out = vec![(
        gl::row(
            "The OMSI 2 folder",
            'a',
            "Choose…",
            &shown(
                &c.root,
                "Not chosen yet: the folder with Omsi.exe, maps and Vehicles in it",
            ),
            None,
        ),
        "setup_root".to_string(),
    )];
    if !core::IN_PROCESS_GAMES {
        out.push((
            gl::row(
                "The game program",
                'a',
                "Choose…",
                &shown(&c.game, "The neoomsi program beside the launcher"),
                None,
            ),
            "setup_game".to_string(),
        ));
    }
    out
}

fn update_rows(l: &Launcher) -> Rows {
    use crate::updater::Status;
    let status = l.update.status();
    let busy = matches!(
        status,
        Status::Checking
            | Status::Downloading { .. }
            | Status::Installing(_)
            | Status::WaitingForInstaller(_)
            | Status::Restarting(_)
    );
    let text = match &status {
        Status::UpToDate => format!(
            "{} is the latest version",
            crate::updater::current_version()
        ),
        Status::Available(rel) => format!("{} is available", rel.version),
        Status::Failed(_) => "The last check failed".to_string(),
        _ => format!("This is neoOMSI {}", crate::updater::current_version()),
    };
    let mut out: Rows = [
        ("update_check", "Look for updates when the launcher starts"),
        ("update_auto", "Install updates without asking"),
    ]
    .iter()
    .filter_map(|(id, name)| gl::switch_row(None, id, name, ""))
    .collect();
    out.push((
        gl::row(
            "Look for an update",
            'a',
            if busy { "" } else { "Check now" },
            &text,
            None,
        ),
        "update_now".to_string(),
    ));
    out.push((
        gl::row(
            "neoOMSI on GitHub",
            'a',
            "Open",
            crate::updater::REPO_URL,
            None,
        ),
        "update_github".to_string(),
    ));
    out
}

fn split(id: &str) -> (&str, &str) {
    id.split_once(' ').unwrap_or((id, ""))
}

fn dropdown_items(
    root: &std::path::Path,
    id: &str,
) -> Option<(Vec<(String, String)>, Option<usize>)> {
    if id == "gfxprofile_del" {
        let items: Vec<_> = ::config::get_subs("graphics_profiles")
            .into_iter()
            .map(|n| (n.clone(), format!("gfxprofile_delete {n}")))
            .collect();
        return (!items.is_empty()).then_some((items, None));
    }
    gl::settings_dropdown(root, 0, id).map(|d| (d.items, d.current))
}

const LABEL_PX: f32 = 13.0;
const DESC_PX: f32 = 12.0;

fn draw_rows(
    ui: &mut Ui,
    v: &mut SettingsView,
    root: &std::path::Path,
    area: Rect,
    rows: &Rows,
    scroll: &str,
) -> Option<Act> {
    let mut out = None;
    ui.scroll_area(scroll, area, &mut |ui, r| {
        let mut y = r.y;
        let ctrl_w = (r.w * 0.42).clamp(160.0, 300.0);
        let text_w = r.w - ctrl_w - GAP - 10.0;
        for (k, (row, id)) in rows.iter().enumerate() {
            let p: Vec<&str> = row.split('\u{1f}').collect();
            let field = |i: usize| p.get(i).copied().unwrap_or("");
            let (name, kind, value, desc) = (
                tr(field(0)),
                field(1).chars().next().unwrap_or('i'),
                tr(field(2)),
                tr(field(3)),
            );
            let frac: f32 = field(4).parse().unwrap_or(0.0);
            let meter: Option<f32> = field(6).parse().ok();
            let one_sided = field(7) == "u";
            let lit = field(8) == "1";
            let name_id = format!("{scroll}-{k}-{id}");
            if kind == 'h' {
                y += if k == 0 { 2.0 } else { 14.0 };
                ui.text_in(
                    &name.to_uppercase(),
                    Rect::new(r.x, y, r.w, 22.0),
                    11.5,
                    Weight::Bold,
                    TEXT_DIM,
                    Align::Left,
                );
                y += 28.0;
                continue;
            }
            let label_h = if name.is_empty() { 0.0 } else { 20.0 };
            let desc_h = if desc.is_empty() {
                0.0
            } else {
                ui.paragraph_height(&desc, text_w, DESC_PX, Weight::Regular)
            };
            let h = (label_h + desc_h + 12.0).max(ROW + 8.0);
            let ctrl = Rect::new(r.right() - ctrl_w - 6.0, y + (h - ROW) * 0.5, ctrl_w, ROW);
            if !name.is_empty() {
                ui.text_in(
                    &name,
                    Rect::new(r.x, y + 6.0, text_w, 18.0),
                    LABEL_PX,
                    Weight::Medium,
                    TEXT,
                    Align::Left,
                );
            }
            if !desc.is_empty() {
                let c = if kind == 'c' { DANGER } else { TEXT_DIM };
                ui.paragraph(
                    &desc,
                    Vec2::new(r.x, y + 6.0 + label_h),
                    text_w,
                    DESC_PX,
                    Weight::Regular,
                    c,
                );
            }
            match kind {
                's' => {
                    let mut on = field(2) == "on";
                    if ui.toggle(
                        &name_id,
                        Rect::new(ctrl.right() - 60.0, ctrl.y, 60.0, ctrl.h),
                        &mut on,
                        "",
                    ) {
                        out = Some(Act::Toggle(id.clone()));
                    }
                }
                'v' => {
                    let n = gl::steps_of(split(id).0).map_or(2, |s| s.len()).max(2);
                    let mut f = frac;
                    let shown = value.clone();
                    if ui.slider(
                        &name_id,
                        ctrl,
                        &mut f,
                        0.0,
                        1.0,
                        1.0 / (n - 1) as f32,
                        "",
                        &|_| shown.clone(),
                    ) {
                        out = Some(Act::Slide(id.clone(), f));
                    }
                }
                'o' if id == "reset" => {
                    let armed = v
                        .reset_armed
                        .is_some_and(|t| t.elapsed().as_secs_f32() < 4.0);
                    let label = if armed {
                        "Click again to reset"
                    } else {
                        "Reset all settings"
                    };
                    if ui.button(
                        &name_id,
                        ctrl,
                        label,
                        Some("restart_alt"),
                        ButtonKind::Danger,
                    ) {
                        if armed {
                            v.reset_armed = None;
                            out = Some(Act::Pick("reset_all".into()));
                        } else {
                            v.reset_armed = Some(Instant::now());
                        }
                    }
                }
                'o' => match dropdown_items(root, id) {
                    Some((items, current)) => {
                        let mut labels: Vec<String> = items.iter().map(|i| i.0.clone()).collect();
                        let mut sel = match current {
                            Some(c) => c,
                            None => {
                                labels.insert(
                                    0,
                                    if value.is_empty() {
                                        "Choose…".into()
                                    } else {
                                        value.clone()
                                    },
                                );
                                0
                            }
                        };
                        let before = sel;
                        if ui.select(&name_id, ctrl, &mut sel, &labels) && sel != before {
                            let at = if current.is_some() {
                                sel
                            } else {
                                sel.wrapping_sub(1)
                            };
                            if let Some(it) = items.get(at) {
                                out = Some(Act::Pick(it.1.clone()));
                            }
                        }
                    }
                    None => {
                        let label = if value.is_empty() {
                            "Open".to_string()
                        } else {
                            value.clone()
                        };
                        if ui.button(
                            &name_id,
                            ctrl,
                            &label,
                            Some("chevron_right"),
                            ButtonKind::Normal,
                        ) {
                            out = Some(Act::Press(id.clone()));
                        }
                    }
                },
                'a' if !value.is_empty() => {
                    if ui.button(&name_id, ctrl, &value, None, ButtonKind::Normal) {
                        out = Some(Act::Press(id.clone()));
                    }
                }
                'k' | 'c' | 'E' => {
                    let capturing = kind == 'E';
                    let b = Rect::new(ctrl.x, ctrl.y, ctrl.w - 40.0, ctrl.h);
                    let label = if capturing {
                        "Press a key… (Esc: cancel)".to_string()
                    } else {
                        value.clone()
                    };
                    let style = if capturing {
                        ButtonKind::Primary
                    } else {
                        ButtonKind::Normal
                    };
                    if ui.button(&name_id, b, &label, Some("keyboard"), style) {
                        out = Some(if capturing {
                            Act::Press("keycancel".into())
                        } else {
                            Act::Capture(id.clone())
                        });
                    }
                    if !capturing
                        && ui.icon_button(
                            &format!("{name_id}-x"),
                            Vec2::new(ctrl.right() - 18.0, ctrl.center().y),
                            14.0,
                            "close",
                            "No key",
                        )
                    {
                        out = Some(Act::Clear(id.clone()));
                    }
                }
                'f' | 'F' => {
                    let target = if id == "gfxprofile_name" {
                        &mut v.profile_name
                    } else {
                        &mut v.key_filter
                    };
                    if ui.text_input(
                        &name_id,
                        ctrl,
                        target,
                        if id == "keysearch" {
                            "Find a key or action"
                        } else {
                            "Name"
                        },
                        None,
                    ) && id == "keysearch"
                    {
                        v.built = None;
                    }
                }
                _ => {
                    if !value.is_empty() {
                        ui.text_in(
                            &value,
                            ctrl,
                            LABEL_PX,
                            Weight::Regular,
                            TEXT_SOFT,
                            Align::Right,
                        );
                    }
                }
            }
            if let Some(m) = meter {
                let bar = Rect::new(ctrl.x, ctrl.bottom() + 2.0, ctrl.w, 4.0);
                ui.p()
                    .rounded(bar, 2.0, ::user_interface::Color::WHITE.alpha(0.08));
                let m = m.clamp(-1.0, 1.0);
                let (a, b): (f32, f32) = if one_sided {
                    (0.0, m)
                } else {
                    (0.5, 0.5 + m * 0.5)
                };
                let (a, b) = (a.min(b), a.max(b));
                ui.p().rounded(
                    Rect::new(bar.x + bar.w * a, bar.y, (bar.w * (b - a)).max(3.0), bar.h),
                    2.0,
                    ACCENT,
                );
            }
            if lit {
                ui.p()
                    .circle(Vec2::new(ctrl.x - 12.0, ctrl.center().y), 4.5, OK);
            }
            y += h + 4.0;
        }
        y - r.y + 8.0
    });
    out
}

fn act(l: &mut Launcher, a: Act) {
    l.state.save_pending_settings();
    l.state.reload_changed_settings();
    let mut msg: Option<String> = None;
    let mut tab: Option<usize> = None;
    match a {
        Act::Toggle(id) => {
            let (verb, arg) = split(&id);
            if verb.starts_with("wiz_") {
                super::pad_wizard::press(
                    &mut l.settings.wizard,
                    l.settings.pads.as_mut(),
                    verb,
                    arg,
                );
            } else if verb.starts_with("pad_") {
                tab = crate::lab_pads::click(l.settings.pads.as_mut(), verb, arg);
            } else {
                gl::option_apply(None, verb, arg, Move::Next);
            }
        }
        Act::Slide(id, f) => {
            let (verb, arg) = split(&id);
            gl::option_apply(None, verb, arg, Move::To(f));
        }
        Act::Pick(action) => {
            let (verb, arg) = split(&action);
            if verb.starts_with("pad_") {
                crate::lab_pads::apply(l.settings.pads.as_mut(), verb, arg);
            } else if verb == "gfxprofile_delete" {
                ::config::remove_sub("graphics_profiles", arg);
                let _ = ::config::save();
                msg = Some(format!("Deleted the graphics profile \"{arg}\"."));
            } else {
                if action.starts_with("pick pax_models ") {
                    l.state.pax_changed = Some(core::install::now_secs());
                }
                msg = gl::settings_pick(None, &action);
            }
        }
        Act::Press(id) => {
            let (verb, arg) = split(&id);
            match verb {
                "seat_reset" => {
                    for k in ["seat_x", "seat_y", "seat_z"] {
                        ::config::set_setting("camera", k, 0.0_f64);
                    }
                    let _ = ::config::save();
                }
                "keycancel" => l.settings.capture = None,
                "pax_pack_get" => {
                    crate::pax_pack::shared(
                        crate::startup::content_dir,
                        crate::pax_pack::PaxPack::start,
                    );
                }
                "pax_restart" => l.state.restart_games(),
                "gfxprofile_save" => msg = Some(save_profile(&mut l.settings.profile_name)),
                "setup_root" => {
                    if let Some(p) =
                        core::pick_folder("The OMSI 2 folder (with maps and Vehicles in it)")
                    {
                        l.state.config.root = p.to_string_lossy().to_string();
                        save_setup(l);
                    }
                }
                "setup_game" => {
                    if let Some(p) = core::pick_file("The neoomsi program") {
                        l.state.config.game = p.to_string_lossy().to_string();
                        save_setup(l);
                    }
                }
                "update_now" => l.update.check(),
                "update_github" => crate::updater::open_url(crate::updater::REPO_URL),
                v if v.starts_with("wiz_") => {
                    super::pad_wizard::press(
                        &mut l.settings.wizard,
                        l.settings.pads.as_mut(),
                        v,
                        arg,
                    );
                }
                v if v.starts_with("pad_") => {
                    tab = crate::lab_pads::click(l.settings.pads.as_mut(), v, arg)
                }
                _ => {}
            }
        }
        Act::Capture(id) => {
            let mut it = id.strip_prefix("keybind ").unwrap_or("").splitn(3, ' ');
            if let (Some(sec), Some(idx), Some(name)) = (
                it.next().and_then(|x| x.parse().ok()),
                it.next().and_then(|x| x.parse().ok()),
                it.next(),
            ) {
                l.settings.capture = Some((sec, idx, name.to_string()));
                l.ui.input.raw_key = None;
            }
        }
        Act::Clear(id) => {
            let mut it = id.strip_prefix("keybind ").unwrap_or("").splitn(3, ' ');
            if let (Some(sec), Some(idx), Some(name)) = (
                it.next().and_then(|x| x.parse().ok()),
                it.next().and_then(|x| x.parse().ok()),
                it.next(),
            ) {
                edit_key(l, sec, idx, name, crate::game_menu::KeyEdit::Clear);
            }
        }
    }
    gl::flush_settings(true);
    l.state.settings_written();
    l.settings.built = None;
    if let Some(t) = tab {
        l.settings.tab = t;
    }
    if let Some(m) = msg {
        l.state.set_status(m, false);
    }
}

fn save_profile(name: &mut String) -> String {
    let n: String = name.trim().chars().take(40).collect();
    if n.is_empty() {
        return "Give the profile a name.".into();
    }
    for (cat, key, _) in ::config::DEFAULTS {
        if *cat == "graphics" {
            if let Some(v) = ::config::get_setting("graphics", key) {
                ::config::set_setting_sub("graphics_profiles", &n, key, v);
            }
        }
    }
    let _ = ::config::save();
    name.clear();
    format!("Saved the graphics profile \"{n}\".")
}

fn save_setup(l: &mut Launcher) {
    match core::save_config(&l.state.config) {
        Ok(()) => {
            ::legacy_config::content_changed();
            l.state.config = core::load_config();
            l.state.load_content();
            l.state.set_status("Saved.", false);
        }
        Err(e) => l.state.set_status(format!("{e:#}"), true),
    }
}
