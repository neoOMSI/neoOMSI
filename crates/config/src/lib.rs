//! Game settings stored as TOML

mod default_settings;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

pub use default_settings::{DEFAULTS, Def};
pub use toml::Value;
use toml::Table;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("toml write: {0}")]
    Write(#[from] toml::ser::Error),
    #[error("no settings path set (call ::config::init first)")]
    NoPath,
}

struct State {
    path: Option<PathBuf>,
    table: Table,
}

fn state() -> MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(|| {
            Mutex::new(State {
                path: None,
                table: defaults(),
            })
        })
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub fn defaults() -> Table {
    let mut t = Table::new();
    for (cat, key, def) in DEFAULTS {
        category_mut(&mut t, cat).insert((*key).to_string(), def.to_value());
    }
    t
}

fn category_mut<'a>(t: &'a mut Table, cat: &str) -> &'a mut Table {
    let e = t
        .entry(cat.to_string())
        .or_insert_with(|| Value::Table(Table::new()));
    if !e.is_table() {
        *e = Value::Table(Table::new());
    }
    e.as_table_mut().unwrap()
}

fn merge(dst: &mut Table, src: Table) {
    for (k, v) in src {
        match (dst.get_mut(&k), v) {
            (Some(Value::Table(d)), Value::Table(s)) => merge(d, s),
            (_, v) => {
                dst.insert(k, v);
            }
        }
    }
}

/// `%APPDATA%\neoOMSI\settings.toml` on Windows, `~/Library/Application Support/neoOMSI`
/// on macOS, `$XDG_CONFIG_HOME` or `~/.config` + `/neoOMSI` elsewhere.
pub fn default_path() -> PathBuf {
    use std::env::var_os;
    let home = || var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let base = if cfg!(windows) {
        var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home)
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support")
    } else {
        var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"))
    };
    base.join("neoOMSI").join("settings.toml")
}

pub fn init(path: impl AsRef<Path>) -> Result<(), ConfigError> {
    let path = path.as_ref().to_path_buf();
    let mut st = state();
    st.path = Some(path);
    drop(st);
    load()
}

pub fn load() -> Result<(), ConfigError> {
    let mut st = state();
    let path = st.path.clone().ok_or(ConfigError::NoPath)?;
    let mut table = defaults();
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let mut saved = text.parse::<Table>()?;
            migrate_window_mode(&mut saved);
            migrate_stick_sens(&mut saved);
            merge(&mut table, saved);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::info!("settings file {} not found, using defaults", path.display());
        }
        Err(e) => return Err(e.into()),
    }
    st.table = table;
    Ok(())
}

fn migrate_stick_sens(table: &mut Table) {
    let Some(controls) = table.get_mut("controls").and_then(Value::as_table_mut) else {
        return;
    };
    if controls.contains_key("stick_sens_v2") {
        return;
    }
    if controls.get("stick_sens").and_then(Value::as_float) == Some(0.25) {
        controls.insert("stick_sens".to_string(), Value::Float(1.0));
    }
}

/// Move the old fullscreen switch to the explicit window-mode setting.
fn migrate_window_mode(table: &mut Table) {
    let Some(graphics) = table.get_mut("graphics").and_then(Value::as_table_mut) else {
        return;
    };
    let fullscreen = graphics.remove("fullscreen").and_then(|v| v.as_bool());
    if !graphics.contains_key("window_mode") && fullscreen == Some(true) {
        graphics.insert("window_mode".to_string(), Value::String("borderless".to_string()));
    }
}

pub fn save() -> Result<(), ConfigError> {
    let st = state();
    let path = st.path.clone().ok_or(ConfigError::NoPath)?;
    let text = toml::to_string_pretty(&st.table)?;
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn get_setting(category: &str, key: &str) -> Option<Value> {
    state().table.get(category)?.get(key).cloned()
}

pub fn set_setting(category: &str, key: &str, value: impl Into<Value>) {
    let mut st = state();
    category_mut(&mut st.table, category).insert(key.to_string(), value.into());
}

pub fn get_setting_sub(category: &str, sub: &str, key: &str) -> Option<Value> {
    state().table.get(category)?.get(sub)?.get(key).cloned()
}

pub fn set_setting_sub(category: &str, sub: &str, key: &str, value: impl Into<Value>) {
    let mut st = state();
    category_mut(category_mut(&mut st.table, category), sub)
        .insert(key.to_string(), value.into());
}

pub fn remove_setting_sub(category: &str, sub: &str, key: &str) {
    let mut st = state();
    if let Some(t) = st
        .table
        .get_mut(category)
        .and_then(|c| c.get_mut(sub))
        .and_then(|s| s.as_table_mut())
    {
        t.remove(key);
    }
}

pub fn get_table_sub(category: &str, sub: &str) -> Vec<(String, Value)> {
    state()
        .table
        .get(category)
        .and_then(|c| c.get(sub))
        .and_then(|s| s.as_table())
        .map(|t| t.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

pub fn get_subs(category: &str) -> Vec<String> {
    state()
        .table
        .get(category)
        .and_then(|c| c.as_table())
        .map(|t| t.iter().filter(|(_, v)| v.is_table()).map(|(k, _)| k.clone()).collect())
        .unwrap_or_default()
}

pub fn remove_sub(category: &str, sub: &str) {
    if let Some(t) = state().table.get_mut(category).and_then(|c| c.as_table_mut()) {
        t.remove(sub);
    }
}

pub fn reset_setting(category: &str, key: &str) {
    let default = DEFAULTS
        .iter()
        .find(|(c, k, _)| *c == category && *k == key)
        .map(|(_, _, d)| d.to_value());
    let mut st = state();
    match default {
        Some(v) => {
            category_mut(&mut st.table, category).insert(key.to_string(), v);
        }
        None => {
            if let Some(t) = st.table.get_mut(category).and_then(|c| c.as_table_mut()) {
                t.remove(key);
            }
        }
    }
}

pub fn reset_all() {
    state().table = defaults();
}

// Typed shortcuts

pub fn get_bool(category: &str, key: &str) -> Option<bool> {
    get_setting(category, key)?.as_bool()
}

pub fn get_int(category: &str, key: &str) -> Option<i64> {
    get_setting(category, key)?.as_integer()
}

pub fn get_float(category: &str, key: &str) -> Option<f64> {
    match get_setting(category, key)? {
        Value::Float(f) => Some(f),
        Value::Integer(i) => Some(i as f64),
        _ => None,
    }
}

pub fn get_string(category: &str, key: &str) -> Option<String> {
    get_setting(category, key)?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_set() {
        reset_all();
        assert_eq!(get_string("graphics", "window_mode"), Some("windowed".to_string()));
        set_setting_sub("controller", "Logitech G29", "Button1", "view_interiorcam_minus");
        assert_eq!(
            get_setting_sub("controller", "Logitech G29", "Button1")
                .and_then(|v| v.as_str().map(str::to_string)),
            Some("view_interiorcam_minus".to_string())
        );
    }

    #[test]
    fn old_fullscreen_setting_becomes_borderless() {
        let mut table: Table = "[graphics]\nfullscreen = true\n".parse().unwrap();
        migrate_window_mode(&mut table);
        let graphics = table["graphics"].as_table().unwrap();
        assert_eq!(graphics.get("window_mode").and_then(Value::as_str), Some("borderless"));
        assert!(!graphics.contains_key("fullscreen"));
    }

    #[test]
    fn old_default_stick_sensitivity_is_reset_once() {
        let mut old: Table = "[controls]\nstick_sens = 0.25\n".parse().unwrap();
        migrate_stick_sens(&mut old);
        assert_eq!(old["controls"]["stick_sens"].as_float(), Some(1.0));

        let mut chosen: Table = "[controls]\nstick_sens = 0.25\nstick_sens_v2 = true\n"
            .parse()
            .unwrap();
        migrate_stick_sens(&mut chosen);
        assert_eq!(chosen["controls"]["stick_sens"].as_float(), Some(0.25));

        let mut own: Table = "[controls]\nstick_sens = 0.6\n".parse().unwrap();
        migrate_stick_sens(&mut own);
        assert_eq!(own["controls"]["stick_sens"].as_float(), Some(0.6));
    }
}
