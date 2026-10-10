//! Key bindings page.

use super::*;

pub(crate) struct KeyView<'a> {
    pub filter: &'a str,
    pub searching: bool,
    pub capture: Option<(usize, usize)>,
    pub scripted: Vec<String>,
}

impl KeyView<'_> {
    pub(crate) fn of(app: &App) -> KeyView<'_> {
        KeyView {
            filter: &app.key_filter,
            searching: app.key_search,
            capture: app.key_capture,
            scripted: app.scripted_names(),
        }
    }
}

pub(crate) fn key_rows(root: &std::path::Path, view: &KeyView) -> Vec<(String, String)> {
    let Some(v) = crate::keys::keybindings() else {
        return vec![(
            row(&tx("pause.page.keys.load_error"), 'i', "", "", None),
            "noop".to_string(),
        )];
    };
    let names = crate::describe::names(
        root,
        &::config::get_string("ui", "language").unwrap_or_else(|| "en".into()),
    );
    let head = |t: &str, n: usize| {
        (
            row(&t.to_uppercase(), 'i', &n.to_string(), "", None),
            HEADING.to_string(),
        )
    };
    let q = view.filter.trim().to_lowercase();
    let mut out = vec![(
        row(
            &tx("pause.page.keys.find"),
            if view.searching { 'E' } else { 'a' },
            view.filter,
            "",
            None,
        ),
        "keysearch".to_string(),
    )];
    let mut all: Vec<(usize, usize, String, i64, i64)> = Vec::new();
    for (sec, key) in ["vehicles", "game"].iter().enumerate() {
        if let Some(a) = v.get(*key).and_then(|a| a.as_array()) {
            for (i, b) in a.iter().enumerate() {
                let action = b
                    .get("action")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if !action.is_empty() {
                    all.push((
                        sec,
                        i,
                        action,
                        b.get("scan_code").and_then(|x| x.as_i64()).unwrap_or(0),
                        b.get("modifier").and_then(|x| x.as_i64()).unwrap_or(0),
                    ));
                }
            }
        }
    }
    let groups: [(
        String,
        Box<dyn Fn(&(usize, usize, String, i64, i64)) -> bool>,
    ); 3] = [
        (tx("pause.page.keys.group_driving"), Box::new(|b| b.0 == 0)),
        (
            tx("pause.page.text.the_game"),
            Box::new(|b| b.0 == 1 && !b.2.starts_with("vr_")),
        ),
        (
            tx("pause.page.keys.group_vr"),
            Box::new(|b| b.0 == 1 && b.2.starts_with("vr_")),
        ),
    ];
    let mut any = false;
    // scripted keybinds
    let scripted: Vec<(usize, String)> = view
        .scripted
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, n)| {
            q.is_empty()
                || view.capture.is_some_and(|c| c.0 == 2)
                || names.key_label(n).to_lowercase().contains(&q)
                || n.to_lowercase().contains(&q)
        })
        .collect();
    if !scripted.is_empty() {
        any = true;
        out.push(head(&tx("pause.page.keys.group_scripted"), scripted.len()));
        for (i, action) in scripted {
            let label = names.key_label(&action);
            let id = format!("keybind 2 {i} {action}");
            if view.capture == Some((2, i)) {
                out.push((
                    row(
                        &label,
                        'E',
                        &tx("pause.page.keys.press_key"),
                        &tx("pause.page.keys.press_key_desc"),
                        None,
                    ),
                    id,
                ));
            } else {
                out.push((
                    row(&label, 'k', &tx("pause.page.keys.not_set"), "", None),
                    id,
                ));
            }
        }
    }
    for (title, pick) in groups.iter() {
        let mut members: Vec<&(usize, usize, String, i64, i64)> = all
            .iter()
            .filter(|b| pick(b))
            .filter(|b| {
                q.is_empty()
                    || view.capture == Some((b.0, b.1))
                    || names.key_label(&b.2).to_lowercase().contains(&q)
                    || b.2.to_lowercase().contains(&q)
                    || crate::keys::key_name(b.3, b.4).to_lowercase().contains(&q)
            })
            .collect();
        if members.is_empty() {
            continue;
        }
        // (alphabetical by the name shown)
        members.sort_by_cached_key(|b| names.key_label(&b.2).to_lowercase());
        any = true;
        out.push(head(title, members.len()));
        for b in members {
            let (sec, i, action, scan, m) = (b.0, b.1, &b.2, b.3, b.4);
            // (every other binding of the same key, in the game's keys and the vehicle's: both are live)
            let clash: Vec<String> = if scan == 0 {
                Vec::new()
            } else {
                all.iter()
                    .filter(|o| (o.0, o.1) != (sec, i) && o.3 == scan && (o.4 & 6) == (m & 6))
                    .map(|o| names.key_label(&o.2))
                    .collect()
            };
            let label = names.key_label(action);
            if view.capture == Some((sec, i)) {
                out.push((
                    row(
                        &label,
                        'E',
                        &tx("pause.page.keys.press_key"),
                        &tx("pause.page.keys.press_key_desc"),
                        None,
                    ),
                    format!("keybind {sec} {i} {action}"),
                ));
                continue;
            }
            let value = if scan == 0 {
                tx("pause.page.keys.not_set")
            } else {
                crate::keys::key_name(scan, m)
            };
            let desc = if clash.is_empty() {
                String::new()
            } else {
                ::i18n::translate("pause.msg.key_conflict", &[("keys", &clash.join(", "))])
            };
            out.push((
                row(&label, 'k', &value, &desc, None),
                format!("keybind {sec} {i} {action}"),
            ));
        }
    }
    if !any {
        out.push((
            row(
                &tx("pause.page.keys.no_match"),
                'i',
                "",
                &tx("pause.page.keys.no_match_desc"),
                None,
            ),
            HEADING.to_string(),
        ));
    }
    out
}
