//! `ailists.cfg`, `unsched_vehgroups.txt`, `unsched_trafficdens.txt`, `parklist_p.txt`,
//! `humans.txt`, `drivers.txt`, `registrations.txt`.

use omsi_cfg::CfgFile;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AiVehicleEntry {
    pub file: String,
    /// Weight (`[aigroup_2]`) or number/registration (`[aigroup_depot_typgroup_2]`).
    pub weight: f32,
    pub number: Option<String>,
    pub registration: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AiGroup {
    pub name: String,
    pub vehicles: Vec<AiVehicleEntry>,
    /// Depot groups: HOF name.
    pub hof: Option<String>,
    pub is_depot: bool,
    /// Depot type groups: vehicle file + list of numbers/registrations.
    pub typgroups: Vec<AiTypGroup>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AiTypGroup {
    pub file: String,
    pub entries: Vec<DepotEntry>,
}

/// One vehicle of a depot's type group: an `[aigroup_depot_typgroup_2]` line is its fleet
/// number, plate, repaint and first and last day (YYYYMMDD), tab-separated (Omsi.exe
/// 0x780a58 splits it at each tab with 0x7ef900, empty fields kept); an
/// `[aigroup_depot_typgroup]` line is the number alone, which also names the repaint.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DepotEntry {
    pub number: String,
    pub registration: String,
    pub paint: String,
    pub from: Option<i32>,
    pub to: Option<i32>,
}

impl DepotEntry {
    /// A `[aigroup_depot_typgroup_2]` line.
    pub fn parse(line: &str) -> DepotEntry {
        let mut f = line.split('\t');
        let mut next = || f.next().unwrap_or("").to_string();
        let (number, registration, paint, from, to) = (next(), next(), next(), next(), next());
        let date = |s: &str| s.trim().parse::<i32>().ok();
        DepotEntry {
            number: number.trim().to_string(),
            registration,
            paint,
            from: date(&from),
            to: date(&to),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AiLists {
    pub groups: Vec<AiGroup>,
}

impl AiLists {
    pub fn load(path: &Path) -> Result<AiLists, omsi_cfg::CfgError> {
        let f = CfgFile::read(path)?;
        Ok(Self::parse(&f))
    }

    pub fn parse(f: &CfgFile) -> AiLists {
        let mut a = AiLists::default();
        // the original lower-cases every line of this file before looking at it
        let mut r = f.reader().with_rule(omsi_cfg::KeywordRule::AnyCase);
        // the old format's list (`[ailist]`): (default group, vehicles)
        let mut legacy: Option<(i32, Vec<String>)> = None;
        while let Some(k) = r.next_keyword() {
            match k.as_str() {
                // the original: a line it passes over, the default group's index, a
                // count and that many vehicle files
                "ailist" => {
                    let _ = r.str();
                    let default = r.str().trim().parse::<i32>().unwrap_or(-1);
                    let n = r.str().trim().parse::<usize>().unwrap_or(0).min(10_000);
                    let files: Vec<String> = (0..n)
                        .map(|_| r.str().trim().to_string())
                        .filter(|f| !f.is_empty())
                        .collect();
                    legacy = Some((default, files));
                }
                "aigroup" | "aigroup_2" => {
                    let name = r.str().to_string();
                    // second header line: the depot (hof) name, empty for plain car groups
                    let hof = r.str().trim().to_string();
                    // (a map that leaves the depot line out altogether starts the list right
                    // there: Novi Sad's "Trucks" group - that line is a vehicle, not a depot)
                    let lower = hof.to_ascii_lowercase();
                    let vehicle =
                        lower.contains(".bus") || lower.contains(".ovh") || lower.contains(".sco");
                    let mut g = AiGroup {
                        name,
                        hof: if hof.is_empty() || vehicle {
                            None
                        } else {
                            Some(hof.clone())
                        },
                        ..Default::default()
                    };
                    if vehicle {
                        let (file, w) = match hof.split_once('\t') {
                            Some((f, w)) => (f.trim().to_string(), omsi_cfg::parse_f32(w)),
                            None => (hof.trim().to_string(), 1.0),
                        };
                        g.vehicles.push(AiVehicleEntry {
                            file,
                            weight: w,
                            number: None,
                            registration: None,
                        });
                    }
                    for l in r.until("[end]") {
                        let l = l.trim_end();
                        if l.trim().is_empty() {
                            continue;
                        }
                        let (file, w) = match l.split_once('\t') {
                            Some((f, w)) => (f.trim().to_string(), omsi_cfg::parse_f32(w)),
                            None => (l.trim().to_string(), 1.0),
                        };
                        g.vehicles.push(AiVehicleEntry {
                            file,
                            weight: w,
                            number: None,
                            registration: None,
                        });
                    }
                    a.groups.push(g);
                }
                "aigroup_depot" => {
                    let name = r.str().to_string();
                    let hof = r.str().to_string();
                    a.groups.push(AiGroup {
                        name,
                        hof: Some(hof),
                        is_depot: true,
                        ..Default::default()
                    });
                }
                "aigroup_depot_typgroup" | "aigroup_depot_typgroup_2" => {
                    let file = r.str().to_string();
                    let mut tg = AiTypGroup {
                        file,
                        entries: Vec::new(),
                    };
                    let v2 = k == "aigroup_depot_typgroup_2";
                    for l in r.until("[end]") {
                        if l.trim().is_empty() {
                            continue;
                        }
                        tg.entries.push(if v2 {
                            DepotEntry::parse(l)
                        } else {
                            DepotEntry {
                                number: l.trim().to_string(),
                                paint: l.trim().to_string(),
                                ..Default::default()
                            }
                        });
                    }
                    if let Some(g) = a.groups.last_mut() {
                        g.typgroups.push(tg);
                    }
                }
                _ => {}
            }
        }
        // no default group given: every listed vehicle that is in no group makes the group
        // "NotInGroup" (weight 1), as OMSI does after reading the file
        if let Some((default, files)) = legacy {
            if default < 0 || default as usize >= a.groups.len() {
                let grouped: std::collections::HashSet<String> = a
                    .groups
                    .iter()
                    .flat_map(|g| g.vehicles.iter().map(|v| v.file.to_ascii_lowercase()))
                    .collect();
                let vehicles: Vec<AiVehicleEntry> = files
                    .into_iter()
                    .filter(|f| !grouped.contains(&f.to_ascii_lowercase()))
                    .map(|file| AiVehicleEntry {
                        file,
                        weight: 1.0,
                        number: None,
                        registration: None,
                    })
                    .collect();
                if !vehicles.is_empty() {
                    a.groups.push(AiGroup {
                        name: "NotInGroup".into(),
                        vehicles,
                        ..Default::default()
                    });
                }
            }
        }
        a
    }
}

/// A plain list file (one entry per line, blank lines ignored).
pub fn load_list(path: &Path) -> Vec<String> {
    match CfgFile::read(path) {
        Ok(f) => f
            .lines
            .iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnschedGroup {
    pub name: String,
    pub factor: f32,
    /// (day-of-week mask, list of (hour, density))
    pub densities: Vec<(i32, Vec<(f32, f32)>)>,
}

/// `unsched_trafficdens.txt`
pub fn parse_unsched_trafficdens(f: &CfgFile) -> Vec<UnschedGroup> {
    let mut out: Vec<UnschedGroup> = Vec::new();
    let mut r = f.reader();
    while let Some(k) = r.next_keyword() {
        match k.as_str() {
            "group" => {
                let name = r.str().to_string();
                let factor = r.f32();
                out.push(UnschedGroup {
                    name,
                    factor,
                    densities: Vec::new(),
                });
            }
            "set_day_of_week" => {
                let d = r.i32();
                if let Some(g) = out.last_mut() {
                    g.densities.push((d, Vec::new()));
                }
            }
            "trafficdensity" => {
                let t = r.f32();
                let d = r.f32();
                if let Some(g) = out.last_mut() {
                    if g.densities.is_empty() {
                        g.densities.push((0, Vec::new()));
                    }
                    g.densities.last_mut().unwrap().1.push((t, d));
                }
            }
            _ => {}
        }
    }
    out
}

/// `unsched_vehgroups.txt`: (group name, default density class)
pub fn parse_unsched_vehgroups(f: &CfgFile) -> Vec<(String, i32)> {
    let mut out = Vec::new();
    let mut r = f.reader();
    while let Some(k) = r.next_keyword() {
        if k == "group" {
            let n = r.str().to_string();
            let d = r.i32();
            out.push((n, d));
        }
    }
    out
}

/// Chrono `Chrono.cfg`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChronoCfg {
    pub start_date: i32,
    pub end_date: i32,
    pub ticket_pack: Option<String>,
    pub money_system: Option<String>,
    pub deactivate_lines: Vec<String>,
}

pub fn parse_chrono_cfg(f: &CfgFile) -> ChronoCfg {
    let mut c = ChronoCfg::default();
    let mut r = f.reader();
    while let Some(k) = r.next_keyword() {
        match k.as_str() {
            "startdate" => c.start_date = r.i32(),
            "enddate" => c.end_date = r.i32(),
            "ticketpack" => c.ticket_pack = Some(r.str().to_string()),
            "moneysystem" => c.money_system = Some(r.str().to_string()),
            // a count, then that many line names (the original: StrToInt, then ReadLn
            // n times). The count is not a line: read as one it took line "1" or "2" off.
            "deactivate_lines" => {
                let n = r.i32().max(0);
                c.deactivate_lines = (0..n)
                    .map(|_| r.str().trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            _ => {}
        }
    }
    c
}

impl ChronoCfg {
    /// Is the scenario in force on `date` (YYYYMMDD)? As the original: a scenario
    /// without any date never is; `[startdate]` and `[enddate]` both count as in force.
    pub fn active_on(&self, date: i32) -> bool {
        (self.start_date != 0 || self.end_date != 0)
            && (self.start_date == 0 || date >= self.start_date)
            && (self.end_date == 0 || date <= self.end_date)
    }
}

/// The `Chrono.cfg` files under a map's `Chrono` folder in the game's order (OMSI
/// the original): depth first, a folder's subfolders before its own files, names in the
/// order Windows lists them (case-insensitive). A later scenario overrides an earlier one.
fn chrono_cfgs(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries = omsi_cfg::vfs::list_dir(dir).unwrap_or_default();
    entries.sort_by_key(|(n, _)| n.to_string_lossy().to_uppercase());
    for (n, is_dir) in &entries {
        if *is_dir {
            chrono_cfgs(&dir.join(n), out);
        }
    }
    for (n, is_dir) in &entries {
        if !*is_dir && n.to_string_lossy().eq_ignore_ascii_case("Chrono.cfg") {
            out.push(dir.join(n));
        }
    }
}

/// Chrono folders of a map that are active on `date` (YYYYMMDD), in the game's order.
pub fn active_chrono_dirs(map_dir: &Path, date: i32) -> Vec<PathBuf> {
    let mut cfgs = Vec::new();
    chrono_cfgs(&omsi_cfg::resolve_path(map_dir, "Chrono"), &mut cfgs);
    cfgs.into_iter()
        .filter(|p| {
            CfgFile::read(p)
                .map(|f| parse_chrono_cfg(&f).active_on(date))
                .unwrap_or(false)
        })
        .filter_map(|p| p.parent().map(|d| d.to_path_buf()))
        .collect()
}

/// The lines the given (active) chrono folders take off the timetable (`[deactivate_lines]`),
/// each with the folder that does it, in folder order. A scenario takes a line off only for
/// the folders before it (the map's own TTData and earlier scenarios): a later one may bring
/// the line back (see `TimetableData::load_with_chrono`).
pub fn chrono_deactivated_lines(chrono_dirs: &[PathBuf]) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for d in chrono_dirs {
        let cfg =
            Some(omsi_cfg::resolve_path(d, "Chrono.cfg")).filter(|p| omsi_cfg::vfs::is_file(p));
        if let Some(f) = cfg.and_then(|p| CfgFile::read(&p).ok()) {
            out.extend(
                parse_chrono_cfg(&f)
                    .deactivate_lines
                    .into_iter()
                    .map(|l| (l, d.clone())),
            );
        }
    }
    out
}

/// The map's `ailists.cfg` with the updates of the given (active) chrono folders
/// (`ailists_#upd.cfg`, else a full `ailists.cfg` of the scenario): a group of the same name
/// gets the new vehicles, and a depot the depot file (`.hof`) the scenario names - Berlin's
/// buses change to "Spandau 1994" on 29 May 1994.
pub fn ailists_with_chrono(map_dir: &Path, chrono_dirs: &[PathBuf]) -> AiLists {
    let mut ailists =
        AiLists::load(&omsi_cfg::resolve_path(map_dir, "ailists.cfg")).unwrap_or_default();
    for c in chrono_dirs {
        for name in ["ailists_#upd.cfg", "ailists.cfg"] {
            if let Ok(extra) = AiLists::load(&c.join(name)) {
                for g in extra.groups {
                    match ailists
                        .groups
                        .iter_mut()
                        .find(|x| x.name.eq_ignore_ascii_case(&g.name) && x.is_depot == g.is_depot)
                    {
                        Some(base) => {
                            base.vehicles.extend(g.vehicles);
                            base.typgroups.extend(g.typgroups);
                            if g.hof.is_some() {
                                base.hof = g.hof;
                            }
                        }
                        None => ailists.groups.push(g),
                    }
                }
                break;
            }
        }
    }
    ailists
}

/// The depot file (`.hof` name) the map's own buses use on `date` (YYYYMMDD): the first
/// depot's, with the chrono scenarios of that date.
pub fn depot_hof_on(map_dir: &Path, date: i32) -> Option<String> {
    let l = ailists_with_chrono(map_dir, &active_chrono_dirs(map_dir, date));
    l.groups
        .iter()
        .filter(|g| g.is_depot)
        .chain(l.groups.iter())
        .find_map(|g| g.hof.clone())
}

/// A date as the game and the launcher write it (`YYYY-MM-DD`) as the chrono's `YYYYMMDD`.
pub fn date_code(date: &str) -> Option<i32> {
    let v: Vec<i32> = date
        .split('-')
        .filter_map(|x| x.trim().parse().ok())
        .collect();
    match v[..] {
        [y, m, d] if (1..=12).contains(&m) && (1..=31).contains(&d) => {
            Some(y * 10000 + m * 100 + d)
        }
        _ => None,
    }
}

/// Whether a depot vehicle is in service on `date` (YYYYMMDD): from its first day, if it has
/// one, to its last, if it has one (0x781a15, 0x781a47).
pub fn typgroup_entry_valid(entry: &DepotEntry, date: i32) -> bool {
    entry.from.is_none_or(|f| f <= date) && entry.to.is_none_or(|t| t >= date)
}

/// `signalroutes.cfg` (unit `mc_fahrstrasse`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SignalRoute {
    pub kind: i32,
    pub signal: (i64, i32),
    pub entries: Vec<[i64; 4]>,
    pub dist_signal: Option<i64>,
    pub next_signal: Option<i64>,
    pub speed_limit: Option<f32>,
}

pub fn parse_signalroutes(f: &CfgFile) -> Vec<SignalRoute> {
    let mut out: Vec<SignalRoute> = Vec::new();
    let mut r = f.reader();
    while let Some(k) = r.next_keyword() {
        match k.as_str() {
            "signalroute" => out.push(SignalRoute {
                kind: r.i32(),
                ..Default::default()
            }),
            "signal" => {
                let a = r.i64();
                let b = r.i32();
                if let Some(s) = out.last_mut() {
                    s.signal = (a, b);
                }
            }
            "entry" => {
                let e = [r.i64(), r.i64(), r.i64(), r.i64()];
                if let Some(s) = out.last_mut() {
                    s.entries.push(e);
                }
            }
            "distsignal" => {
                let v = r.i64();
                if let Some(s) = out.last_mut() {
                    s.dist_signal = Some(v);
                }
            }
            "nextsignal" => {
                let v = r.i64();
                if let Some(s) = out.last_mut() {
                    s.next_signal = Some(v);
                }
            }
            "speedlimit" => {
                let v = r.f32();
                if let Some(s) = out.last_mut() {
                    s.speed_limit = Some(v);
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_codes() {
        assert_eq!(date_code("2026-09-17"), Some(20260917));
        assert_eq!(date_code("1989-5-30"), Some(19890530));
        assert_eq!(date_code("1989-13-30"), None);
        assert_eq!(date_code(""), None);
    }

    /// Spandau's 1991 timetable change takes line "5 & 5N" off: on any later date the chrono
    /// that does it is active and names the line; before it, nothing takes the line off.
    #[test]
    fn spandau_takes_line_5_off_in_1991() {
        let root = std::env::var_os("OMSI_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../../../OMSI 2 Original"));
        let map = root.join("maps/Berlin-Spandau");
        if !map.join("Chrono").is_dir() {
            eprintln!("skipped: no {}", map.display());
            return;
        }
        let later = chrono_deactivated_lines(&active_chrono_dirs(&map, 20260917));
        let by = later
            .iter()
            .find(|(l, _)| l == "5 & 5N")
            .map(|(_, d)| d.file_name().unwrap().to_string_lossy().into_owned());
        assert_eq!(by.as_deref(), Some("1000_FPW_19910602"), "{later:?}");
        let before = chrono_deactivated_lines(&active_chrono_dirs(&map, 19890530));
        assert!(!before.iter().any(|(l, _)| l == "5 & 5N"), "{before:?}");
        // the count before the names is no line
        assert!(
            !later.iter().any(|(l, _)| l == "1" || l == "17" || l == "2"),
            "{later:?}"
        );
    }

    #[test]
    fn chrono_dates_are_inclusive_and_need_one() {
        let c = |s, e| ChronoCfg {
            start_date: s,
            end_date: e,
            ..Default::default()
        };
        assert!(!c(0, 0).active_on(19900101));
        assert!(c(19900910, 19900923).active_on(19900923));
        assert!(!c(19900910, 19900923).active_on(19900924));
        assert!(c(0, 19870430).active_on(19800101));
        assert!(c(19870501, 0).active_on(20260101));
    }
}

#[cfg(test)]
mod legacy_tests {
    #[test]
    fn the_old_ailist_makes_a_group_of_its_own() {
        let f = omsi_cfg::CfgFile::from_str(
            "ailists.cfg",
            "[ailist]\n0\n-1\n2\nvehicles\\A\\a.bus\nvehicles\\B\\b.ovh\n",
        );
        let a = super::AiLists::parse(&f);
        assert_eq!(a.groups.len(), 1);
        assert_eq!(a.groups[0].name, "NotInGroup");
        assert_eq!(a.groups[0].vehicles.len(), 2);
    }
}

#[cfg(test)]
mod chrono_hof_tests {
    #[test]
    fn berlin_changes_its_depot_file_with_the_date() {
        let dir = std::path::Path::new("../../../OMSI 2 Original/maps/Berlin-Spandau");
        if !dir.is_dir() {
            return;
        }
        let at = |d| super::depot_hof_on(dir, d).unwrap_or_default();
        assert_eq!(at(19940601), "Spandau 1994");
        assert_ne!(at(19870101), "Spandau 1994");
        assert_eq!(at(19891201), "Spandau 1989-12");
    }
}
