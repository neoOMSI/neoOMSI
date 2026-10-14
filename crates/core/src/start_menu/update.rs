use super::*;
use crate::updater::{self, Status};

fn update_view(u: &updater::Updater) -> ui::StartUpdate {
    let (kind, label, progress) = match u.status() {
        Status::Available(r) if !u.dismissed => (1, format!("Update {}", r.version), 0.0),
        Status::Downloading { release, done, total } => (
            2,
            format!("{}  {}%", release.version, if total > 0 { done * 100 / total } else { 0 }),
            done as f32 / total.max(1) as f32,
        ),
        Status::Installing(_) | Status::Restarting(_) | Status::WaitingForInstaller(_) => {
            (3, "Installing...".to_string(), 1.0)
        }
        Status::Failed(m) if !u.dismissed && !m.starts_with("Could not check") => {
            (4, "Update failed - retry".to_string(), 0.0)
        }
        _ => (0, String::new(), 0.0),
    };
    ui::StartUpdate { kind, label, progress }
}

impl App {
    pub(super) fn start_update_tick(&mut self) {
        let Some(m) = self.menu.as_mut() else {
            return;
        };
        if !m.update.checked_once {
            if ::legacy_config::env::var_os("OMSI_NO_UPDATE").is_none()
                && ::config::get_bool("launcher", "update_check").unwrap_or(true)
            {
                let _ = updater::just_updated();
                updater::cleanup_after_update();
                m.update.check();
            } else {
                m.update.checked_once = true;
            }
        }
        m.update.poll();
        if let Status::Available(r) = m.update.status() {
            if ::config::get_bool("launcher", "update_auto").unwrap_or(false)
                && !m.update.dismissed
                && !m.update.auto_started
            {
                m.update.auto_started = true;
                log::info!("update: installing {} by itself (update_auto)", r.version);
                m.update.install(r);
            }
        }
        if let Status::Restarting(r) = m.update.status() {
            if !m.update.relaunched {
                m.update.relaunched = true;
                match updater::install_place().and_then(|p| updater::relaunch(&p)) {
                    Ok(()) => {
                        log::info!("update: {} installed, the new program starts", r.version);
                        quit::request();
                    }
                    Err(e) => m.update.fail(format!(
                        "neoOMSI {} is installed; start it again yourself ({e}).",
                        r.version
                    )),
                }
            }
        }
    }

    pub(super) fn start_update_view(&self) -> ui::StartUpdate {
        self.menu.as_ref().map(|m| update_view(&m.update)).unwrap_or_default()
    }

    pub(super) fn start_update_click(&mut self) {
        let Some(m) = self.menu.as_mut() else {
            return;
        };
        match m.update.status() {
            Status::Available(r) => m.update.install(r),
            Status::Failed(_) => m.update.check(),
            _ => {}
        }
    }
}
