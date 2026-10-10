use ::content::input::{self, KeyboardCfg};
use launcher_protocol::api::{self, *};

impl From<crate::Config> for Config {
    fn from(c: crate::Config) -> Config {
        Config {
            root: c.root,
            game: c.game,
            profile: c.profile,
        }
    }
}

impl From<crate::MapInfo> for MapInfo {
    fn from(m: crate::MapInfo) -> MapInfo {
        MapInfo {
            name: m.name,
            friendly: m.friendly,
            file: m.file,
            description: m.description,
            entry_points: m
                .entry_points
                .into_iter()
                .map(|e| EntryInfo {
                    index: e.index,
                    name: e.name,
                })
                .collect(),
            hof: m.hof,
            installed: m.installed,
        }
    }
}

impl From<crate::VehicleInfo> for VehicleInfo {
    fn from(v: crate::VehicleInfo) -> VehicleInfo {
        VehicleInfo {
            name: v.name,
            manufacturer: v.manufacturer,
            type_name: v.type_name,
            file: v.file,
            folder: v.folder,
            description: v.description,
            default_paint: v.default_paint,
            paints: v.paints,
            hofs: v.hofs,
            installed: v.installed,
            missing_packs: v.missing_packs,
            numbers: v
                .numbers
                .into_iter()
                .map(|(number, plate)| FleetNumber { number, plate })
                .collect(),
        }
    }
}

impl From<crate::WeatherInfo> for WeatherInfo {
    fn from(w: crate::WeatherInfo) -> WeatherInfo {
        WeatherInfo {
            name: w.name,
            file: w.file,
            description: w.description,
            fog_m: w.fog_m,
            temp: w.temp,
            clouds: w.clouds,
            precip: w.precip,
            snow: w.snow,
            installed: w.installed,
        }
    }
}

impl From<crate::LineInfo> for LineInfo {
    fn from(l: crate::LineInfo) -> LineInfo {
        let trip = |t: crate::TripInfo| TripInfo {
            name: t.name,
            index: t.index as u32,
            line: t.line,
            from: t.from,
            terminus: t.terminus,
            departure: t.departure,
            arrival: t.arrival,
            stops: t
                .stops
                .into_iter()
                .map(|s| StopInfo {
                    name: s.name,
                    arr: s.arr,
                    dep: s.dep,
                })
                .collect(),
            km: t.km,
        };
        LineInfo {
            name: l.name,
            user_allowed: l.user_allowed,
            termini: l.termini,
            tours: l
                .tours
                .into_iter()
                .map(|t| TourInfo {
                    number: t.number,
                    ai_group: t.ai_group,
                    first: t.first,
                    last: t.last,
                    days: t.days,
                    runs: t.runs,
                    next_run: t.next_run,
                    trips: t.trips.into_iter().map(trip).collect(),
                })
                .collect(),
        }
    }
}

impl From<crate::IbisInfo> for IbisInfo {
    fn from(i: crate::IbisInfo) -> IbisInfo {
        IbisInfo {
            hof: i.hof,
            line_code: i.line_code,
            routes: i
                .routes
                .into_iter()
                .map(|r| IbisRoute {
                    code: r.code,
                    route: r.route,
                    name: r.name,
                    terminus_code: r.terminus_code,
                    terminus: r.terminus,
                })
                .collect(),
        }
    }
}

impl From<crate::Session> for Session {
    fn from(s: crate::Session) -> Session {
        Session {
            time: s.time,
            driver: s.driver,
            map: s.map,
            bus: s.bus,
            line: s.line,
            tour: s.tour,
            seconds: s.seconds,
            metres: s.metres,
            stops: s.stops,
            early: s.early,
            late: s.late,
            tickets: s.tickets,
            cash: s.cash,
            crashes: s.crashes,
            hurt: s.hurt,
            jolts: s.jolts,
            driving: s.driving,
            comfort: s.comfort,
            ticketing: s.ticketing,
        }
    }
}

impl From<crate::Profile> for Profile {
    fn from(p: crate::Profile) -> Profile {
        Profile {
            name: p.name,
            file: p.file,
            hours: p.hours,
            km: p.km,
            xp: p.xp,
            level: p.level,
            next_level_xp: p.next_level_xp,
            stops: p.stops,
            early: p.early,
            late: p.late,
            tickets: p.tickets,
            cash: p.cash,
            crashes: p.crashes,
            hurt: p.hurt,
            rating_driving: p.rating_driving,
            rating_comfort: p.rating_comfort,
            rating_tickets: p.rating_tickets,
            sessions: p.sessions.into_iter().map(Into::into).collect(),
            exists: p.exists,
        }
    }
}

impl From<crate::install::InstallMode> for InstallMode {
    fn from(m: crate::install::InstallMode) -> InstallMode {
        match m {
            crate::install::InstallMode::Extract => InstallMode::Extract,
            crate::install::InstallMode::InPlace => InstallMode::InPlace,
            crate::install::InstallMode::Auto => InstallMode::Auto,
        }
    }
}

pub fn install_mode(m: InstallMode) -> &'static str {
    match m {
        InstallMode::Extract => "extract",
        InstallMode::InPlace => "inplace",
        InstallMode::Unspecified | InstallMode::Auto => "auto",
    }
}

impl From<crate::install::Progress> for InstallProgress {
    fn from(p: crate::install::Progress) -> InstallProgress {
        let state = match p.state.as_str() {
            "queued" => InstallState::Queued,
            "planning" => InstallState::Planning,
            "checking" => InstallState::Checking,
            "unpacking" => InstallState::Unpacking,
            "copying" => InstallState::Copying,
            "moving" => InstallState::Moving,
            "done" => InstallState::Done,
            "failed" => InstallState::Failed,
            "cancelled" => InstallState::Cancelled,
            _ => InstallState::Unspecified,
        };
        InstallProgress {
            id: p.id,
            source: p.source,
            name: p.name,
            state: state.into(),
            mode: InstallMode::from(p.mode).into(),
            files_done: p.files_done,
            files_total: p.files_total,
            bytes_done: p.bytes_done,
            bytes_total: p.bytes_total,
            free_bytes: p.free_bytes,
            needed_bytes: p.needed_bytes,
            message: p.message,
            report: p.report,
            warnings: p.warnings,
            installed: p.installed,
            kept_aside: p.kept_aside,
            from_inbox: p.from_inbox,
            started: p.started,
            finished: p.finished,
        }
    }
}

impl From<crate::install::SourceInfo> for SourceInfo {
    fn from(s: crate::install::SourceInfo) -> SourceInfo {
        SourceInfo {
            is_archive: s.is_archive,
            is_zip: s.is_zip,
            files: s.files,
            unpacked_bytes: s.unpacked_bytes,
            archive_bytes: s.archive_bytes,
            needed_bytes: s.needed_bytes,
            free_bytes: s.free_bytes,
            fits: s.fits,
            in_place: s.in_place,
            in_place_ok: s.in_place_ok,
            suggested: InstallMode::from(s.suggested).into(),
        }
    }
}

impl From<crate::ModsStatus> for ModsStatus {
    fn from(m: crate::ModsStatus) -> ModsStatus {
        ModsStatus {
            content_dir: m.content_dir,
            folders: m
                .folders
                .into_iter()
                .map(|(folder, entries)| FolderCount {
                    folder,
                    entries: entries as u64,
                })
                .collect(),
            inbox: m.inbox,
            inbox_items: m.inbox_items,
            waiting: m.waiting,
            archives: m
                .archives
                .into_iter()
                .map(|(name, bytes)| ArchiveSize { name, bytes })
                .collect(),
            free_bytes: m.free_bytes,
            cleaned: m.cleaned,
            jobs: m.jobs.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<crate::Instance> for Instance {
    fn from(i: crate::Instance) -> Instance {
        Instance {
            id: i.id,
            pid: i.pid,
            process_started: i.process_started,
            slot: i.slot,
            log: i.log,
            started: i.started,
            map: i.map,
            bus: i.bus,
            entry: i.entry,
            line: i.line,
            tour: i.tour,
            profile: i.profile,
            lan: i.lan,
            args: i.args,
            running: i.running,
            ended: i.ended,
            exit_code: i.exit_code,
            stopping: i.stopping,
            killed: i.killed,
            lan_status: i.lan_status.as_ref().map(api::lan_status),
            last_line: i.last_line,
            link: i.link,
        }
    }
}

impl From<Duty> for crate::Duty {
    fn from(d: Duty) -> crate::Duty {
        crate::Duty {
            map: d.map,
            bus: d.bus,
            paint: d.paint,
            plate: d.plate,
            number: d.number,
            hof: d.hof,
            entry: d.entry,
            line: d.line,
            tour: d.tour,
            trip: d.trip,
            whole_tour: d.whole_tour,
            time: d.time,
            date: d.date,
            weather: d.weather,
            traffic: d.traffic,
            passengers: d.passengers,
            schedule: d.schedule,
            autostart: d.autostart,
            on_foot: d.on_foot,
            profile: d.profile,
            lan: d.lan,
            lan_name: d.lan_name,
            season: d.season,
            tutorial: d.tutorial.map(|t| t as usize),
            situation: d.situation,
            ..Default::default()
        }
    }
}

impl From<crate::Launched> for Launched {
    fn from(l: crate::Launched) -> Launched {
        Launched {
            pid: l.pid,
            log: l.log,
            command: l.command,
            others: l.others as u32,
        }
    }
}

pub fn key_bindings(k: KeyboardCfg) -> KeyBindings {
    let list = |l: Vec<input::KeyBinding>| {
        l.into_iter()
            .map(|b| KeyBinding {
                action: b.action,
                scan_code: b.scan_code,
                modifier: b.modifier,
            })
            .collect()
    };
    KeyBindings {
        game: list(k.game),
        vehicles: list(k.vehicles),
    }
}

pub fn keyboard_cfg(k: KeyBindings) -> KeyboardCfg {
    let list = |l: Vec<KeyBinding>| {
        l.into_iter()
            .map(|b| input::KeyBinding {
                action: b.action,
                scan_code: b.scan_code,
                modifier: b.modifier,
            })
            .collect()
    };
    KeyboardCfg {
        game: list(k.game),
        vehicles: list(k.vehicles),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_of_the_table_is_in_the_protocol() {
        let mut table: Vec<_> = crate::setting_keys().collect();
        let mut fields: Vec<_> = api::SETTING_FIELDS
            .iter()
            .copied()
            .filter(|f| *f != "texture_memory_auto")
            .collect();
        table.sort();
        fields.sort();
        assert_eq!(table, fields);
    }

    #[test]
    fn install_states_and_modes_keep_their_meaning() {
        let p = InstallProgress::from(crate::install::Progress {
            state: "unpacking".into(),
            mode: crate::install::InstallMode::InPlace,
            finished: Some(7),
            ..Default::default()
        });
        assert_eq!(
            (p.state(), p.mode(), p.finished),
            (InstallState::Unpacking, InstallMode::InPlace, Some(7))
        );
        assert_eq!(install_mode(InstallMode::Unspecified), "auto");
        assert_eq!(
            crate::install::InstallMode::parse(install_mode(InstallMode::InPlace)),
            crate::install::InstallMode::InPlace
        );
    }
}
