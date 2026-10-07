use chrono::Local;
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogType {
    Info,
    Warn,
    Error,
    Crash,
    Debug,
}

impl LogType {
    pub fn name(self) -> &'static str {
        match self {
            LogType::Info => "Info",
            LogType::Warn => "Warn",
            LogType::Error => "Error",
            LogType::Crash => "Crash",
            LogType::Debug => "Debug",
        }
    }
}

struct Logger {
    path: PathBuf,
    level: LevelFilter,
    file: Mutex<Option<File>>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

pub fn log_dir() -> PathBuf {
    config::default_path()
        .parent()
        .map(|p| p.join("logs"))
        .unwrap_or_else(|| PathBuf::from("logs"))
}

fn level_from_env() -> LevelFilter {
    std::env::var("RUST_LOG")
        .ok()
        .and_then(|v| {
            v.split(',')
                .find(|s| !s.contains('='))
                .and_then(|s| s.trim().parse().ok())
        })
        .unwrap_or(LevelFilter::Info)
}

pub fn init(name: &str) {
    let logger = LOGGER.get_or_init(|| {
        let stamp = Local::now().format("%Y-%m-%d_%H_%M_%S");
        Logger {
            path: log_dir().join(format!("{name}-{stamp}.log")),
            level: level_from_env(),
            file: Mutex::new(None),
        }
    });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(logger.level);
    }
}

pub fn crash(category: Option<&str>, message: &str) {
    match LOGGER.get() {
        Some(l) => l.write(LogType::Crash, category, message),
        None => {
            let _ = writeln!(
                std::io::stderr(),
                "{}",
                format_line(LogType::Crash, category, message)
            );
        }
    }
}

fn format_line(ty: LogType, category: Option<&str>, message: &str) -> String {
    let now = Local::now().format("%d.%m.%Y %H:%M:%S");
    match category {
        Some(c) if !c.is_empty() => format!("[{now}] [{}] [{c}] {message}", ty.name()),
        _ => format!("[{now}] [{}] {message}", ty.name()),
    }
}

impl Logger {
    fn write(&self, ty: LogType, category: Option<&str>, message: &str) {
        let line = format_line(ty, category, message);
        let _ = writeln!(std::io::stderr(), "{line}");
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if file.is_none() {
            if let Some(dir) = self.path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            *file = File::create(&self.path).ok();
        }
        if let Some(f) = file.as_mut() {
            let _ = writeln!(f, "{line}");
            if ty == LogType::Crash {
                let _ = f.sync_all();
            }
        }
    }
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= self.level
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let ty = match r.level() {
            Level::Error => LogType::Error,
            Level::Warn => LogType::Warn,
            Level::Info => LogType::Info,
            Level::Debug | Level::Trace => LogType::Debug,
        };
        let category = if r.module_path() == Some(r.target()) {
            None
        } else {
            Some(r.target())
        };
        self.write(ty, category, &r.args().to_string());
    }

    fn flush(&self) {
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = file.as_mut() {
            let _ = f.flush();
        }
    }
}