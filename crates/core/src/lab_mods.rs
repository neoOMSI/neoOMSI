//! The Mods page of the options. The start menu applies a change at once; a running game keeps
//! the mods it started with until the next start.

use crate::App;
use crate::game_lists::{Cache, HEADING, button, cached, row};
use ::mods::diagnostics::{Finding, Kind, Severity};
use ::mods::{Conflict, ModEntry, ModKind, NewMods};
use omsi_launcher_lib::{MapReferences, install};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Rows = Vec<(String, String)>;

pub(crate) const MISSING_TAB: &str = "pause.options.mods.missing.tab";

fn t(key: &str) -> String {
    ::i18n::translate(&format!("pause.options.mods.{key}"), &[])
}

fn tp(key: &str, params: &[(&str, &dyn std::fmt::Display)]) -> String {
    ::i18n::translate(&format!("pause.options.mods.{key}"), params)
}

fn error_text(error: anyhow::Error) -> String {
    format!("{error:#}")
}

fn info(name: &str, desc: &str) -> (String, String) {
    (row(name, 'i', "", desc, None), "noop".to_string())
}

fn heading(text: &str) -> (String, String) {
    (row(text, 'h', "", "", None), HEADING.to_string())
}

pub(crate) fn applies_now(app: Option<&App>) -> bool {
    app.is_none_or(|app| app.menu.is_some())
}

type Slot<K, V> = Mutex<Option<(K, Option<Result<V, String>>)>>;

/// Runs `work` on a background thread, returning `None` until it's done. One thread at a time.
fn in_background<K, V>(
    slot: &'static Slot<K, V>,
    key: K,
    work: impl FnOnce() -> Result<V, String> + Send + 'static,
) -> Option<Result<V, String>>
where
    K: PartialEq + Clone + Send + 'static,
    V: Clone + Send + 'static,
{
    let mut entry = slot.lock().unwrap_or_else(|e| e.into_inner());
    match entry.as_ref() {
        Some((entry_key, value)) if *entry_key == key => return value.clone(),
        Some((_, None)) => return None,
        _ => {}
    }
    *entry = Some((key.clone(), None));
    drop(entry);
    std::thread::spawn(move || {
        let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
            .unwrap_or_else(|_| Err("it stopped with an error, see the log".into()));
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, Some(value)));
    });
    None
}

static CONFLICTS: Slot<u64, Vec<Conflict>> = Mutex::new(None);
static PREFLIGHT: Slot<(String, u64), Vec<Finding>> = Mutex::new(None);
static MODS: Cache<(bool, u64, usize), Option<Arc<Mods>>> = Mutex::new(None);
static MISSING: Cache<bool, Rows> = Mutex::new(None);
static NOTICE: Mutex<Option<(Instant, String, bool)>> = Mutex::new(None);
const NOTICE_SECS: f32 = 10.0;
static PENDING_UNINSTALL: Mutex<Option<(Instant, String)>> = Mutex::new(None);
const CONFIRM_SECS: f32 = 30.0;
/// A running game saved a change that only takes effect at the next start.
static NEXT_START: AtomicBool = AtomicBool::new(false);
static DRIVE_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

struct Mods {
    error: Option<String>,
    saved: Option<NewMods>,
    list: Vec<Mod>,
}

struct Mod {
    name: String,
    state: State,
}

enum State {
    Missing,
    Unreadable(String),
    /// No OMSI 2 content folder in it.
    Empty,
    /// Whether its plugins are allowed.
    Plugins(bool),
    Fine,
}

fn mods(applies_now: bool) -> Option<Arc<Mods>> {
    let finished = install::jobs().iter().filter(|job| job.finished.is_some()).count();
    let key = (applies_now, ::legacy_config::content_generation(), finished);
    cached(&MODS, key, Duration::from_secs(5), || {
        // (registering again picks up what was installed or copied in by hand)
        let content = if applies_now {
            omsi_launcher_lib::register_content()
        } else {
            omsi_launcher_lib::content_dir()
        }?;
        let order = ::mods::load_order(&content);
        let list = order
            .items
            .iter()
            .map(|item| Mod {
                name: item.name.clone(),
                state: order
                    .entry(&item.name)
                    .filter(|_| item.available)
                    .map_or(State::Missing, state),
            })
            .collect();
        Some(Arc::new(Mods {
            error: order.error.clone(),
            saved: order.saved.as_ref().map(|saved| saved.new_mods),
            list,
        }))
    })
}

fn state(entry: &ModEntry) -> State {
    if let Some(error) = ::mods::mount_error(&entry.path) {
        return State::Unreadable(error);
    }
    if !has_content(entry) {
        return State::Empty;
    }
    if omsi_launcher_lib::mod_has_plugins(&entry.path) {
        return State::Plugins(omsi_launcher_lib::mod_plugins_allowed(&entry.path));
    }
    State::Fine
}

fn invalidate() {
    *MODS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *MISSING.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub(crate) fn rows(app: Option<&App>) -> Rows {
    let applies_now = applies_now(app);
    if applies_now {
        // (the start menu registers the order it shows)
        NEXT_START.store(false, Ordering::Relaxed);
    }
    let Some(mods) = mods(applies_now) else {
        return vec![info(&t("no_content.name"), &t("no_content.desc"))];
    };
    let mut out = notice_rows();
    out.extend(job_rows());
    out.extend(order_rows(&mods, applies_now));
    out
}

/// Only computed while the tab is visible, because checking a map pins every CPU core.
pub(crate) fn missing(app: Option<&App>) -> Rows {
    let Some(app) = app else {
        return Rows::new();
    };
    let applies_now = applies_now(Some(app));
    let title = if applies_now {
        let Some((name, _)) = app.menu.as_ref().and_then(|menu| menu.maps.get(menu.map)) else {
            return Rows::new();
        };
        tp("missing.map", &[("map", name)])
    } else {
        t("missing.game")
    };
    if !app.lab_sub_shown(MISSING_TAB) {
        return vec![heading(&title)];
    }
    cached(&MISSING, applies_now, Duration::from_secs(1), || {
        let mut rows = notice_rows();
        rows.push(heading(&title));
        rows.extend(missing_rows(app, applies_now));
        rows.extend(hidden_row());
        rows
    })
}

fn notice_rows() -> Rows {
    let notice = NOTICE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    notice
        .filter(|(at, ..)| at.elapsed().as_secs_f32() < NOTICE_SECS)
        .map(|(_, text, is_error)| {
            let value = if is_error { t("jobs.state.failed") } else { String::new() };
            vec![(row(&text, 'i', &value, &text, None), "noop".to_string())]
        })
        .unwrap_or_default()
}

fn job_rows() -> Rows {
    let jobs = install::jobs();
    if jobs.is_empty() {
        return Rows::new();
    }
    let mut out = vec![heading(&t("jobs.heading"))];
    for job in &jobs {
        let key = format!("pause.options.mods.jobs.state.{}", job.state);
        let state = Some(::i18n::translate(&key, &[]))
            .filter(|text| *text != key)
            .unwrap_or_else(|| job.state.clone());
        if job.finished.is_none() {
            let pct = (job.bytes_done * 100).checked_div(job.bytes_total).unwrap_or(0);
            let value = tp("jobs.progress", &[("state", &state), ("pct", &pct)]);
            out.push((row(&job.name, 'o', &value, "", None), format!("mod_job {}", job.id)));
        } else {
            let detail = if job.state == "done" {
                tp("jobs.installed", &[("folder", &job.installed.join(", "))])
            } else {
                job.message.clone()
            };
            out.push((row(&job.name, 'i', &state, &detail, None), "noop".to_string()));
        }
    }
    if jobs.iter().any(|job| job.finished.is_some()) {
        out.push(button(&t("jobs.clear"), &t("jobs.clear_button"), "", "mod_jobs_clear"));
    }
    out
}

fn order_rows(mods: &Mods, applies_now: bool) -> Rows {
    let conflicts = in_background(&CONFLICTS, ::legacy_config::content_generation(), || {
        Ok(::mods::conflicts())
    })
    .and_then(Result::ok)
    .unwrap_or_default();
    let conflict_of = |id: &str| conflicts.iter().find(|conflict| conflict.id == id);
    let pending = pending_uninstall();
    let mut problems = 0;
    let mut lines = Rows::new();
    for (index, m) in mods.list.iter().enumerate() {
        let conflict = conflict_of(&m.name);
        let hidden = conflict.is_some_and(fully_hidden);
        let (mut value, mut desc) = match &m.state {
            State::Missing => (t("state.missing"), vec![t("order.unavailable")]),
            State::Unreadable(error) => (t("state.error"), vec![tp("unreadable", &[("error", error)])]),
            State::Empty => (t("state.empty"), vec![t("status.empty_desc")]),
            State::Plugins(_) | State::Fine if hidden => (t("state.hidden"), Vec::new()),
            State::Plugins(false) => (t("state.plugins_blocked"), vec![t("plugins.blocked")]),
            State::Plugins(true) => (String::new(), vec![t("plugins.allowed")]),
            State::Fine => (String::new(), Vec::new()),
        };
        if !value.is_empty() && !matches!(m.state, State::Plugins(false)) {
            problems += 1;
        }
        desc.extend(conflict.map(conflict_notes).unwrap_or_default());
        if pending.as_deref() == Some(m.name.as_str()) {
            value = t("uninstall.ask");
            desc.push(t("uninstall.desc"));
        }
        // (only a lone mod in a running game can have an empty drop-down, see `dropdown`)
        let has_actions =
            applies_now || mods.list.len() > 1 || matches!(m.state, State::Missing | State::Plugins(_));
        lines.push((
            row(
                &format!("{}. {}", index + 1, m.name),
                if has_actions { 'o' } else { 'i' },
                &value,
                &desc.join("\n"),
                None,
            ),
            format!("mod {}", m.name),
        ));
    }
    let mut status = Vec::new();
    if problems > 0 {
        status.push(tp("order.problems", &[("n", &problems)]));
    }
    if !applies_now && NEXT_START.load(Ordering::Relaxed) {
        status.push(t("order.next_start"));
    }
    let mut out = vec![(
        row(&t("order.heading"), 'h', &status.join(" · "), "", None),
        HEADING.to_string(),
    )];
    if let Some(error) = &mods.error {
        out.push(info(&t("order.unreadable"), &tp("order.unreadable_desc", &[("error", error)])));
    }
    if mods.list.is_empty() {
        out.push(info(&t("none"), &t("none_desc")));
    }
    out.extend(lines);
    let mut shared_desc = vec![t("shared.desc")];
    shared_desc.extend(conflict_of(::mods::SHARED_DIR).map(conflict_notes).unwrap_or_default());
    let shared_desc = shared_desc.join("\n");
    out.push(button(&t("shared.name"), &t("shared.button"), &shared_desc, "mod_shared_open"));
    if !mods.list.is_empty() {
        let new_mods = mods.saved.unwrap_or_default();
        out.push((
            row(&t("new_mods.name"), 'o', &t(new_mods_key(new_mods)), &t("new_mods.desc"), None),
            "mod_new_mods".to_string(),
        ));
        if mods.saved.is_some() {
            out.push((
                row(&t("reset.name"), 'o', &t("reset.button"), &t("reset.desc"), None),
                "mod_reset_ask".to_string(),
            ));
        }
    }
    out
}

fn new_mods_key(new_mods: NewMods) -> &'static str {
    match new_mods {
        NewMods::Top => "new_mods.top",
        NewMods::Bottom => "new_mods.bottom",
    }
}

fn pending_uninstall() -> Option<String> {
    let pending = PENDING_UNINSTALL.lock().unwrap_or_else(|e| e.into_inner()).clone();
    pending
        .filter(|(at, _)| at.elapsed().as_secs_f32() < CONFIRM_SECS)
        .map(|(_, name)| name)
}

fn has_content(entry: &ModEntry) -> bool {
    let root = match entry.kind {
        ModKind::Folder => ::mods::folder_content_root(&entry.path),
        ModKind::Zip => match ::legacy_config::vfs::mount_zip(&entry.path) {
            Ok(root) => root,
            Err(_) => return false,
        },
    };
    ::legacy_config::vfs::list_dir(&root).is_some_and(|names| {
        names.iter().any(|(name, is_dir)| {
            *is_dir
                && ::legacy_config::CONTENT_FOLDERS
                    .iter()
                    .any(|folder| name.to_string_lossy().eq_ignore_ascii_case(folder))
        })
    })
}

fn fully_hidden(conflict: &Conflict) -> bool {
    conflict.files > 0 && conflict.overridden == conflict.files
}

fn conflict_notes(conflict: &Conflict) -> Vec<String> {
    let mut out = Vec::new();
    if fully_hidden(conflict) {
        out.push(tp("order.hidden", &[("mods", &conflict.hidden_by.join(", "))]));
    } else if conflict.overridden > 0 {
        out.push(tp(
            "order.overridden",
            &[("n", &conflict.overridden), ("mods", &conflict.hidden_by.join(", "))],
        ));
    }
    if conflict.overrides > 0 {
        out.push(tp(
            "order.overrides",
            &[("n", &conflict.overrides), ("mods", &conflict.overrides_mods.join(", "))],
        ));
    }
    if conflict.replaces_original > 0 {
        out.push(tp("order.replaces", &[("n", &conflict.replaces_original)]));
    }
    for clash in &conflict.clashes {
        out.push(tp("order.clash", &[("folder", &clash.folder), ("mod", &clash.other)]));
    }
    out
}

/// The start menu adds the last drive's misses to the map check: some, like a vehicle's textures,
/// only show up while driving.
fn missing_rows(app: &App, applies_now: bool) -> Rows {
    let undismissed = omsi_launcher_lib::instances::undismissed;
    if !applies_now {
        return finding_rows(undismissed(::mods::diagnostics::findings()));
    }
    let Some((_, map)) = app.menu.as_ref().and_then(|menu| menu.maps.get(menu.map)).cloned()
    else {
        return Rows::new();
    };
    let drive_findings = last_drive(&map);
    let never_driven = drive_findings.is_none();
    let key = (map.clone(), ::legacy_config::content_generation());
    let mut out = match in_background(&PREFLIGHT, key, move || preflight(&map)) {
        None => vec![info(&t("missing.checking"), "")],
        Some(Err(error)) => vec![info(&t("missing.failed"), &error)],
        Some(Ok(mut found)) => {
            ::mods::diagnostics::merge(&mut found, drive_findings.unwrap_or_default());
            finding_rows(undismissed(found))
        }
    };
    if never_driven {
        out.push(info(&t("missing.not_driven.name"), &t("missing.not_driven.desc")));
    }
    out
}

fn preflight(map: &str) -> Result<Vec<Finding>, String> {
    let refs = omsi_launcher_lib::map_references(map).map_err(error_text)?;
    let mut wanted = missing_textures(&refs);
    wanted.extend(refs.wanted);
    Ok(::mods::diagnostics::check(&refs.map_dir, &wanted))
}

/// Found the same way `scene::upload` does, without loading the map.
fn missing_textures(refs: &MapReferences) -> Vec<(PathBuf, Kind)> {
    use rayon::prelude::*;
    let mut paths: Vec<PathBuf> = refs
        .named
        .par_iter()
        .flat_map_iter(|(rel, kind)| match kind {
            Kind::Object => object_textures(&refs.root, rel),
            Kind::Spline => spline_textures(&refs.root, rel),
            _ => Vec::new(),
        })
        .collect();
    paths.sort();
    paths.dedup();
    paths.into_iter().map(|path| (path, Kind::Texture)).collect()
}

/// Skips slots for generated text and script images, whose names are placeholders.
fn object_textures(root: &Path, rel: &str) -> Vec<PathBuf> {
    let path = ::mods::resolve_path(root, rel);
    let (Ok(sco), Some(sco_dir)) = (::scenery::SceneryObject::load(&path), path.parent()) else {
        return Vec::new();
    };
    let (model, model_dir) = match &sco.model_file {
        Some(file) => {
            let model_path = ::mods::resolve_path(sco_dir, file);
            match (::model::Model::load(&model_path), model_path.parent()) {
                (Ok(model), Some(dir)) => (model, dir.to_path_buf()),
                _ => return Vec::new(),
            }
        }
        None => (sco.model.clone(), sco_dir.to_path_buf()),
    };
    let mut names = Vec::new();
    for mesh_def in &model.meshes {
        // (the game reports missing meshes itself)
        let Ok(mesh) = ::legacy_o3d::load_mesh(&crate::scene::mesh_path(&model_dir, &mesh_def.file))
        else {
            continue;
        };
        for material in &mesh.materials {
            let generated = mesh_def.materials.iter().any(|material_def| {
                material_def.texture.trim().eq_ignore_ascii_case(material.texture.trim())
                    && (material_def.use_text_texture.is_some()
                        || material_def.use_script_texture.is_some()
                        || material_def.freetex.is_some())
            });
            if !generated {
                names.push(material.texture.clone());
            }
        }
        for material_def in &mesh_def.materials {
            names.extend(material_def.envmap.iter().map(|(file, _)| file.clone()));
            names.extend(
                material_def
                    .envmap_mask
                    .iter()
                    .chain(&material_def.transmap)
                    .chain(&material_def.nightmap)
                    .cloned(),
            );
        }
    }
    unfound(names, &crate::scene::texture_dirs(root, &model_dir))
}

fn spline_textures(root: &Path, rel: &str) -> Vec<PathBuf> {
    let path = ::mods::resolve_path(root, rel);
    let (Ok(spline), Some(dir)) = (::scenery::Spline::load(&path), path.parent()) else {
        return Vec::new();
    };
    let names = spline.textures.iter().map(|texture| texture.file.clone()).collect();
    unfound(names, &crate::scene::texture_dirs(root, dir))
}

/// Names missing from all `dirs`, joined to the first one, where the game reports them.
fn unfound(names: Vec<String>, dirs: &[PathBuf]) -> Vec<PathBuf> {
    let searched: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
    names
        .iter()
        .map(|name| name.trim())
        .filter(|name| crate::scene::names_texture(name))
        .filter(|name| ::texture::find_texture(name, &searched).is_none())
        .filter_map(|name| Some(dirs.first()?.join(name)))
        .collect()
}

/// This session's findings if it drove `map`, else the ones saved last time `map` was driven.
fn last_drive(map: &str) -> Option<Vec<Finding>> {
    let path = omsi_launcher_lib::instances::map_diagnostics_path(map);
    if DRIVE_FILE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() == Some(&path) {
        return Some(::mods::diagnostics::findings());
    }
    serde_json::from_slice(&std::fs::read(&path).ok()?).ok()
}

pub(crate) fn drive_starts(map: &str) {
    save_drive();
    ::mods::diagnostics::clear();
    *DRIVE_FILE.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(omsi_launcher_lib::instances::map_diagnostics_path(map));
}

/// Saves the drive's findings per map (for the start menu) and per instance (for the launcher).
pub(crate) fn save_drive() -> Vec<Finding> {
    let found = ::mods::diagnostics::findings();
    let drive_file = DRIVE_FILE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    for path in drive_file.into_iter().chain(crate::lan::instance_file("diagnostics")) {
        if let Err(error) = ::mods::diagnostics::write(&path, &found) {
            log::warn!("{}: {error}", path.display());
        }
    }
    found
}

fn severity_key(severity: Severity) -> &'static str {
    match severity {
        Severity::Required => "missing.severity.required",
        Severity::Degraded => "missing.severity.degraded",
        Severity::Optional => "missing.severity.optional",
    }
}

fn finding_rows(mut found: Vec<Finding>) -> Rows {
    if found.is_empty() {
        return vec![info(&t("missing.none"), "")];
    }
    found.sort_by_key(|finding| std::cmp::Reverse(finding.severity));
    found
        .iter()
        .map(|finding| {
            let class = finding.class.as_str();
            let (pack, provider) = (
                finding.pack.as_deref().unwrap_or_default(),
                finding.provider.as_deref().unwrap_or_default(),
            );
            let params: [(&str, &dyn std::fmt::Display); 4] = [
                ("requester", &finding.requester),
                ("pack", &pack),
                ("provider", &provider),
                ("count", &finding.count),
            ];
            let desc: Vec<String> = [
                tp(&format!("missing.{class}"), &params),
                tp(&format!("missing.advice.{class}"), &params),
                String::new(),
            ]
            .into_iter()
            .chain(finding.samples.iter().cloned())
            .collect();
            (
                row(
                    &tp(&format!("missing.name.{class}"), &params),
                    'o',
                    &t(severity_key(finding.severity)),
                    &desc.join("\n"),
                    None,
                ),
                format!("mod_finding {}", finding.key()),
            )
        })
        .collect()
}

fn hidden_row() -> Option<(String, String)> {
    let hidden =
        ::mods::diagnostics::dismissed(&omsi_launcher_lib::instances::dismissed_diagnostics_path());
    (!hidden.is_empty()).then(|| {
        button(
            &tp("missing.hidden", &[("n", &hidden.len())]),
            &t("missing.show_hidden"),
            "",
            "mod_undismiss",
        )
    })
}

/// `None` for a row of another page. Confirmation menus list Cancel first. A running game offers no
/// uninstall, because it still reads the mod's files.
pub(crate) fn dropdown(id: &str, applies_now: bool) -> Option<(Rows, Option<usize>)> {
    let item = |key: &str, action: String| (t(key), action);
    let (verb, arg) = id.split_once(' ').unwrap_or((id, ""));
    Some(match verb {
        "mod_new_mods" => {
            let current = mods(applies_now)?.saved.unwrap_or_default();
            (
                vec![
                    item("new_mods.top", "mod_new_mods top".to_string()),
                    item("new_mods.bottom", "mod_new_mods bottom".to_string()),
                ],
                Some(usize::from(current == NewMods::Bottom)),
            )
        }
        "mod_reset_ask" => (
            vec![item("action.cancel", "noop".to_string()), item("reset.confirm", "mod_reset".to_string())],
            None,
        ),
        "mod_job" => (
            vec![
                item("jobs.keep", "noop".to_string()),
                item("jobs.cancel", format!("mod_job_cancel {arg}")),
            ],
            None,
        ),
        "mod_finding" => (vec![item("missing.dismiss", format!("mod_dismiss {arg}"))], None),
        "mod" if pending_uninstall().as_deref() == Some(arg) => (
            vec![
                item("action.cancel", "mod_uninstall_cancel".to_string()),
                item("action.uninstall", format!("mod_uninstall {arg}")),
            ],
            None,
        ),
        "mod" => {
            let mods = mods(applies_now)?;
            let index = mods.list.iter().position(|m| m.name == arg)?;
            let last = index + 1 == mods.list.len();
            // (steps first: Enter takes the first choice at once, and a step is easy to undo)
            let mut items = Vec::new();
            if index > 0 {
                items.push(item("action.up", format!("mod_up {arg}")));
            }
            if !last {
                items.push(item("action.down", format!("mod_down {arg}")));
            }
            if index > 0 {
                items.push(item("action.top", format!("mod_top {arg}")));
            }
            if !last {
                items.push(item("action.bottom", format!("mod_bottom {arg}")));
            }
            match mods.list[index].state {
                State::Missing => items.push(item("action.forget", format!("mod_forget {arg}"))),
                ref state => {
                    if let State::Plugins(allowed) = state {
                        items.push(if *allowed {
                            item("action.block_plugins", format!("mod_plugins off {arg}"))
                        } else {
                            item("action.allow_plugins", format!("mod_plugins on {arg}"))
                        });
                    }
                    if applies_now {
                        items.push(item("action.uninstall_ask", format!("mod_uninstall_ask {arg}")));
                    }
                }
            }
            (items, None)
        }
        _ => return None,
    })
}

/// Returns the error for the launcher; the in-game menu shows it at the top of the page.
pub(crate) fn apply(app: Option<&App>, verb: &str, arg: &str) -> Option<String> {
    *PENDING_UNINSTALL.lock().unwrap_or_else(|e| e.into_inner()) =
        (verb == "mod_uninstall_ask").then(|| (Instant::now(), arg.to_string()));
    let result = act(verb, arg);
    let applies_now = applies_now(app);
    let changed = result == Ok(true);
    if changed && !applies_now {
        NEXT_START.store(true, Ordering::Relaxed);
    }
    let error = result.err();
    if app.is_some() {
        let notice = match &error {
            Some(error) => Some((error.clone(), true)),
            None if changed && !applies_now => Some((t("msg.next_start"), false)),
            None => None,
        };
        *NOTICE.lock().unwrap_or_else(|e| e.into_inner()) =
            notice.map(|(text, is_error)| (Instant::now(), text, is_error));
    }
    invalidate();
    error
}

pub(crate) fn apply_in_game(app: &mut App, verb: &str, arg: &str) {
    if apply(Some(app), verb, arg).is_some() {
        if let Some(u) = app.ui.as_mut() {
            u.world_scroll = 0;
        }
        return;
    }
    match verb {
        "mod_top" | "mod_up" | "mod_down" | "mod_bottom" => {
            app.lab_reopen_drop(&format!("mod {arg}"), verb)
        }
        "mod_uninstall_ask" => app.lab_reopen_drop(&format!("mod {arg}"), ""),
        _ => {}
    }
}

fn target(verb: &str, from: usize, len: usize) -> usize {
    match verb {
        "mod_top" => 0,
        "mod_up" => from.saturating_sub(1),
        "mod_down" => (from + 1).min(len - 1),
        _ => len - 1,
    }
}

/// Returns `Ok(true)` if the action changed which mods the game loads.
fn act(verb: &str, arg: &str) -> Result<bool, String> {
    let content = omsi_launcher_lib::content_dir().ok_or_else(|| t("no_content.desc"))?;
    let order = ::mods::load_order(&content);
    let revision = ::mods::order::revision(&order);
    let mut names: Vec<String> = order.items.iter().map(|item| item.name.clone()).collect();
    let new_mods = order.saved.as_ref().map(|saved| saved.new_mods).unwrap_or_default();
    match verb {
        "mod_top" | "mod_up" | "mod_down" | "mod_bottom" => {
            let Some(from) = names.iter().position(|name| name == arg) else {
                return Ok(false);
            };
            let name = names.remove(from);
            names.insert(target(verb, from, names.len() + 1), name);
            ::mods::order::set(&content, &names, new_mods, revision)?;
        }
        "mod_new_mods" => {
            let placed = if arg == "bottom" { NewMods::Bottom } else { NewMods::Top };
            ::mods::order::set(&content, &names, placed, revision)?;
        }
        "mod_reset" => ::mods::order::reset(&content, revision)?,
        "mod_forget" => ::mods::order::forget(&content, arg)?,
        "mod_plugins" => {
            let (on, name) = arg.split_once(' ').unwrap_or((arg, ""));
            omsi_launcher_lib::allow_mod_plugins(name, on == "on").map_err(error_text)?;
        }
        "mod_uninstall" => {
            omsi_launcher_lib::uninstall_mod(arg).map_err(error_text)?;
        }
        "mod_job_cancel" => {
            install::cancel(arg.parse().unwrap_or(0));
            return Ok(false);
        }
        "mod_jobs_clear" => {
            install::clear_finished();
            return Ok(false);
        }
        "mod_shared_open" => {
            let dir = content.join(::mods::MODS_DIR).join(::mods::SHARED_DIR);
            let _ = std::fs::create_dir_all(&dir);
            crate::updater::open_folder(&dir);
            return Ok(false);
        }
        "mod_dismiss" => {
            omsi_launcher_lib::dismiss_diagnostic(arg).map_err(error_text)?;
            return Ok(false);
        }
        "mod_undismiss" => {
            let path = omsi_launcher_lib::instances::dismissed_diagnostics_path();
            if let Err(error) = std::fs::remove_file(&path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                return Err(format!("{}: {error}", path.display()));
            }
            return Ok(false);
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// A zip too big for the free disk space is used as it is. A mod of the same name is replaced
/// (moved to `mods/uninstalled`), except in a running game, which still reads its files.
pub(crate) fn install(app: Option<&App>) {
    let Some(content) = omsi_launcher_lib::content_dir() else {
        return;
    };
    let Some(files) = rfd::FileDialog::new()
        .set_title(t("install.title"))
        .add_filter("Mod archive", &["zip", "7z", "rar"])
        .pick_files()
    else {
        return;
    };
    let replace = applies_now(app);
    for file in files {
        install::start(content.clone(), file, install::InstallMode::Auto, replace);
    }
    *NOTICE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    invalidate();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_stay_in_the_list() {
        assert_eq!(target("mod_top", 2, 3), 0);
        assert_eq!(target("mod_up", 0, 3), 0);
        assert_eq!(target("mod_up", 2, 3), 1);
        assert_eq!(target("mod_down", 2, 3), 2);
        assert_eq!(target("mod_down", 0, 3), 1);
        assert_eq!(target("mod_bottom", 0, 3), 2);
    }

    #[test]
    fn only_textures_that_are_not_there_are_reported() {
        let dir = std::env::temp_dir().join(format!("lab-mods-unfound-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("here.dds"), b"").unwrap();
        let names = ["here.dds", "gone.png", "0.3", "null.bmp", " "].map(String::from).to_vec();
        assert_eq!(unfound(names, &[dir.clone()]), vec![dir.join("gone.png")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_thread_at_a_time_and_a_panic_ends_it() {
        static SLOT: Slot<u32, u32> = Mutex::new(None);
        let wait_for = |key: u32, work: fn() -> Result<u32, String>| loop {
            if let Some(value) = in_background(&SLOT, key, work) {
                return value;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let (go, wait) = std::sync::mpsc::channel::<()>();
        assert_eq!(in_background(&SLOT, 1, move || Ok(wait.recv().map_or(0, |_| 10))), None);
        assert_eq!(in_background(&SLOT, 2, || Ok(20)), None);
        go.send(()).unwrap();
        assert_eq!(wait_for(1, || Ok(0)), Ok(10), "the busy worker kept its key");
        assert_eq!(wait_for(2, || Ok(20)), Ok(20));
        assert!(wait_for(3, || panic!("broken")).is_err());
    }

    #[test]
    fn an_uninstall_waits_for_its_confirmation() {
        apply(None, "mod_uninstall_ask", "Bus");
        let (items, _) = dropdown("mod Bus", true).unwrap();
        assert_eq!(items[0].1, "mod_uninstall_cancel", "the way back comes first");
        assert_eq!(items[1].1, "mod_uninstall Bus");
        apply(None, "mod_uninstall_cancel", "");
        assert_eq!(pending_uninstall(), None);
    }
}
