//! Row builders shared by the pages.

use super::*;

pub(super) fn tx(key: &str) -> String {
    ::i18n::translate(key, &[])
}

pub(crate) fn row(name: &str, kind: char, value: &str, desc: &str, frac: Option<f32>) -> String {
    format!(
        "{name}\u{1f}{kind}\u{1f}{value}\u{1f}{desc}\u{1f}{}",
        frac.map(|f| format!("{f:.3}")).unwrap_or_default()
    )
}

pub(crate) fn opens(name: &str, desc: &str, id: &str) -> (String, String) {
    (row(name, 'o', "", desc, None), id.to_string())
}

pub(crate) fn button(name: &str, text: &str, desc: &str, id: &str) -> (String, String) {
    (row(name, 'a', text, desc, None), id.to_string())
}

pub(crate) fn switch_row(app: Option<&App>, id: &str, name: &str, desc: &str) -> Option<(String, String)> {
    let on = toggle_now(app, id)?;
    Some((
        row(name, 's', if on { "on" } else { "off" }, desc, None),
        id.to_string(),
    ))
}

pub(crate) fn slider_row(
    app: Option<&App>,
    id: &str,
    name: &str,
    desc: &str,
    fmt: &dyn Fn(f32) -> String,
) -> Option<(String, String)> {
    let (verb, arg) = id.split_once(' ').unwrap_or((id, ""));
    let steps = steps_of(verb)?;
    let now = option_now(app, verb, arg)?;
    let i = nearest(&steps, now);
    let frac = if steps.len() > 1 {
        i as f32 / (steps.len() - 1) as f32
    } else {
        0.0
    };
    Some((row(name, 'v', &fmt(now), desc, Some(frac)), id.to_string()))
}

pub(crate) const MAP_TAB: usize = 99;
pub(crate) const KEYS_TAB: usize = 98;
pub(crate) const LOOK_TAB: usize = 97;

pub(crate) fn is_sub_tab(t: usize) -> bool {
    t == MAP_TAB || t == KEYS_TAB || t == LOOK_TAB
}
