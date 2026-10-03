//! `.hof` depot files: termini, bus stop display strings, IBIS info system.

use omsi_cfg::CfgFile;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Terminus {
    pub code: i32,
    /// Texture change id.
    pub texture_id: String,
    /// Bus stop where everybody leaves (None for `_allexit`).
    pub terminus_stop: Option<String>,
    pub all_exit: bool,
    pub strings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BusStop {
    pub ident: String,
    pub strings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct InfoTrip {
    pub code: String,
    pub name: String,
    pub route: String,
    pub line: String,
    pub extra: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hof {
    pub path: PathBuf,
    pub name: String,
    pub service_trip: String,
    pub global_strings: Vec<String>,
    pub string_count_terminus: usize,
    pub string_count_busstop: usize,
    pub termini: Vec<Terminus>,
    pub bus_stops: Vec<BusStop>,
    pub info_trips: Vec<InfoTrip>,
    pub info_busstop_lists: Vec<Vec<String>>,
    pub info_busstops: Vec<Vec<String>>,
}

impl Hof {
    pub fn load(path: &Path) -> Result<Hof, omsi_cfg::CfgError> {
        let f = CfgFile::read(path)?;
        Ok(Self::parse(&f))
    }

    /// Only the `[name]` of a depot file ("" when it has none), kept for the session: the
    /// searches by name below read every depot file of every vehicle folder, and parsing
    /// each whole (termini, stops, the IVU trips) made a big installation's start take
    /// minutes.
    pub fn read_name(path: &Path) -> Option<String> {
        type Names = std::collections::HashMap<PathBuf, Option<String>>;
        static CACHE: std::sync::OnceLock<std::sync::Mutex<(u64, Names)>> =
            std::sync::OnceLock::new();
        let cache = CACHE.get_or_init(Default::default);
        let generation = omsi_cfg::content_generation();
        {
            let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
            if c.0 != generation {
                *c = (generation, Names::new());
            }
            if let Some(n) = c.1.get(path) {
                return n.clone();
            }
        }
        let name = CfgFile::read(path).ok().map(|f| {
            let mut r = f.reader().with_rule(omsi_cfg::KeywordRule::TrimEnd);
            while let Some(k) = r.next_keyword() {
                if k == "name" {
                    return r.str().to_string();
                }
            }
            String::new()
        });
        cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .insert(path.to_path_buf(), name.clone());
        name
    }

    pub fn parse(f: &CfgFile) -> Hof {
        let mut h = Hof {
            path: f.path.clone(),
            string_count_terminus: 0,
            string_count_busstop: 0,
            ..Default::default()
        };
        // `stringcount_terminus` / `stringcount_busstop` are bare (unbracketed) directives.
        for (i, l) in f.lines.iter().enumerate() {
            let w = l.trim_end();
            if w.eq_ignore_ascii_case("stringcount_terminus") {
                h.string_count_terminus = f
                    .lines
                    .get(i + 1)
                    .map(|s| omsi_cfg::parse_i64(s).max(0) as usize)
                    .unwrap_or(0);
            } else if w.eq_ignore_ascii_case("stringcount_busstop") {
                h.string_count_busstop = f
                    .lines
                    .get(i + 1)
                    .map(|s| omsi_cfg::parse_i64(s).max(0) as usize)
                    .unwrap_or(0);
            }
        }
        // the stock depot files are spreadsheet exports: every line ends in tabs, which the
        // original cuts off (with spaces and quotes) before looking at it
        let mut r = f.reader().with_rule(omsi_cfg::KeywordRule::TrimEnd);
        while let Some(k) = r.next_keyword() {
            match k.as_str() {
                "name" => h.name = r.str().to_string(),
                "servicetrip" => h.service_trip = r.str().to_string(),
                "global_strings" => {
                    let n = r.usize();
                    h.global_strings = (0..n).map(|_| r.str().to_string()).collect();
                }
                "addterminus" | "addterminus_allexit" => {
                    // Despite the SDK comment, no shipped file carries a separate terminus
                    // station line: every record is code, ident, then stringcount strings
                    // (verified over all stock .hof files). The ident doubles as the station.
                    let all_exit = k.ends_with("allexit");
                    let code = r.i32();
                    let texture_id = r.str().to_string();
                    let terminus_stop = if all_exit {
                        None
                    } else {
                        Some(texture_id.clone())
                    };
                    let strings = (0..h.string_count_terminus)
                        .map(|_| r.str().to_string())
                        .collect();
                    h.termini.push(Terminus {
                        code,
                        texture_id,
                        terminus_stop,
                        all_exit,
                        strings,
                    });
                }
                "addterminus_list" => {
                    // One row per terminus, tab separated: a flag column (`{ALLEX}` or
                    // empty), the code, the ident and the display strings - every one of the
                    // 3 549 rows of the stock and installed depot files has the flag column,
                    // empty ones included. Reading the code from the flag column gave every
                    // terminus without `{ALLEX}` the code 0 and its code as ident: typed
                    // destination codes were "wrong", a route found no terminus and the
                    // displays fell back to the first (empty) entry.
                    for l in r.until("[end]") {
                        if l.trim().is_empty() {
                            continue;
                        }
                        let cols: Vec<&str> = l.split('\t').collect();
                        let all_exit = cols[0].trim().eq_ignore_ascii_case("{ALLEX}");
                        let code = omsi_cfg::parse_i32(cols.get(1).unwrap_or(&"0"));
                        let texture_id = cols.get(2).unwrap_or(&"").trim().to_string();
                        let terminus_stop = if all_exit {
                            None
                        } else {
                            Some(texture_id.clone())
                        };
                        let mut strings: Vec<String> =
                            cols.iter().skip(3).map(|s| s.to_string()).collect();
                        if h.string_count_terminus > 0 {
                            strings.resize(h.string_count_terminus, String::new());
                        }
                        h.termini.push(Terminus {
                            code,
                            texture_id,
                            terminus_stop,
                            all_exit,
                            strings,
                        });
                    }
                }
                "addbusstop" => {
                    let ident = r.str().to_string();
                    let strings = (0..h.string_count_busstop)
                        .map(|_| r.str().to_string())
                        .collect();
                    h.bus_stops.push(BusStop { ident, strings });
                }
                "addbusstop_list" => {
                    for l in r.until("[end]") {
                        if l.trim().is_empty() {
                            continue;
                        }
                        let mut cols = l.split('\t');
                        let ident = cols.next().unwrap_or("").trim().to_string();
                        h.bus_stops.push(BusStop {
                            ident,
                            strings: cols.map(|s| s.to_string()).collect(),
                        });
                    }
                }
                "infosystem_trip" => {
                    let code = r.str().to_string();
                    let name = r.str().to_string();
                    let route = r.str().to_string();
                    let line = r.str().to_string();
                    h.info_trips.push(InfoTrip {
                        code,
                        name,
                        route,
                        line,
                        extra: Vec::new(),
                    });
                    // every trip has a stop list, empty until one follows (THof.LoadFromFile
                    // 0x7ea142), so that the lists stay in step with the trips
                    h.info_busstop_lists.push(Vec::new());
                }
                "infosystem_busstop_list" => {
                    // the list of the trip read last (0x7ea16f: DynArrayHigh of the trips);
                    // pushed as one more list, a trip without one (the IVU data routes of
                    // some depot files) gave every later trip the stops of the one before
                    let n = r.usize();
                    let list: Vec<String> = (0..n).map(|_| r.str().to_string()).collect();
                    if let Some(last) = h.info_busstop_lists.last_mut() {
                        *last = list;
                    }
                }
                "infosystem_busstop" => h
                    .info_busstops
                    .push((0..3).map(|_| r.str().to_string()).collect()),
                _ => {}
            }
        }
        h
    }

    pub fn terminus_by_code(&self, code: i32) -> Option<&Terminus> {
        self.termini.iter().find(|t| t.code == code)
    }
}

/// The `.hof` files of a folder, sorted by name: every content root's copy of the folder
/// together (a mod's depot next to the installation's; archives read in place too), a
/// higher-priority root's file hiding the same name lower down.
pub fn depot_files(dir: &Path) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for d in omsi_cfg::mirrored_dirs(dir) {
        for p in omsi_cfg::vfs::read_dir_paths(&d) {
            if !p
                .extension()
                .map(|e| e.eq_ignore_ascii_case("hof"))
                .unwrap_or(false)
            {
                continue;
            }
            if seen.insert(
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase(),
            ) {
                files.push(p);
            }
        }
    }
    files.sort_by_key(|f| {
        f.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
    });
    files
}

/// The depot file of `dir` called `name`: by its file name (without `.hof`) or its `[name]`.
pub fn depot_in(dir: &Path, name: &str) -> Option<Hof> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let files = depot_files(dir);
    if let Some(f) = files.iter().find(|f| {
        f.file_stem()
            .map(|s| s.to_string_lossy().trim().eq_ignore_ascii_case(name))
            .unwrap_or(false)
    }) {
        if let Ok(h) = Hof::load(f) {
            return Some(h);
        }
    }
    files
        .iter()
        .filter(|f| Hof::read_name(f).is_some_and(|n| n.trim().eq_ignore_ascii_case(name)))
        .find_map(|f| Hof::load(f).ok())
}

/// The depot file called `name` in any vehicle folder of any content root (`Vehicles/*/`).
///
/// A depot file belongs to a map, not to a bus model: it lists the map's termini, stops and
/// IBIS codes. A mod bus brings only the depot of the map it was made on (the O530 Citaro
/// pack has Grundorf.hof alone), and on another map every code of the timetable was then
/// unknown to its IBIS - no line and no destination on any display. OMSI players copy the
/// map's .hof into such a folder; this finds the copy that is already installed with
/// another bus.
pub fn depot_anywhere(name: &str) -> Option<Hof> {
    // (asked for every type of AI bus without the map's depot: the file found is kept for
    // the session, and a big installation's thousands of folders are gone through once)
    type Found = std::collections::HashMap<String, Option<PathBuf>>;
    static FOUND: std::sync::OnceLock<std::sync::Mutex<(u64, Found)>> = std::sync::OnceLock::new();
    let found = FOUND.get_or_init(Default::default);
    let key = name.trim().to_ascii_lowercase();
    let generation = omsi_cfg::content_generation();
    let known = {
        let mut f = found.lock().unwrap_or_else(|e| e.into_inner());
        if f.0 != generation {
            *f = (generation, Found::new());
        }
        f.1.get(&key).cloned()
    };
    if let Some(path) = known {
        return path.and_then(|p| Hof::load(&p).ok());
    }
    // every vehicle folder once over all roots (depot_in looks at each root's copy)
    let mut dirs: Vec<PathBuf> = omsi_cfg::read_dir_merged("Vehicles")
        .into_iter()
        .filter(|d| omsi_cfg::vfs::is_dir(d))
        .collect();
    dirs.sort_by_key(|d| {
        d.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
    });
    let h = dirs.iter().find_map(|d| depot_in(d, name));
    found
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .1
        .insert(key, h.as_ref().map(|h| h.path.clone()));
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminus_list_columns() {
        let text = "stringcount_terminus\r\n3\r\n\r\n[addterminus_list]\r\n{ALLEX}\t13\tBetriebsfahrt\tBETRIEBSFAHRT\t\tBETRIEBSFAHRT\t\t\r\n\t282\tU Ruhleben\tRUHLEBEN\tU-BAHNHOF\tRUHLEBEN  \t\t\t\r\n[end]\r\n";
        let h = Hof::parse(&CfgFile::from_str("test.hof", text));
        assert_eq!(h.termini.len(), 2);
        assert_eq!(
            (
                h.termini[0].code,
                h.termini[0].texture_id.as_str(),
                h.termini[0].all_exit
            ),
            (13, "Betriebsfahrt", true)
        );
        assert_eq!(
            h.termini[0].strings,
            vec!["BETRIEBSFAHRT", "", "BETRIEBSFAHRT"]
        );
        assert_eq!(
            (
                h.termini[1].code,
                h.termini[1].texture_id.as_str(),
                h.termini[1].all_exit
            ),
            (282, "U Ruhleben", false)
        );
        assert_eq!(h.termini[1].terminus_stop.as_deref(), Some("U Ruhleben"));
        assert_eq!(
            h.termini[1].strings,
            vec!["RUHLEBEN", "U-BAHNHOF", "RUHLEBEN  "]
        );
        assert_eq!(
            h.terminus_by_code(282).map(|t| t.texture_id.as_str()),
            Some("U Ruhleben")
        );
    }

    /// #667: a trip without a stop list (an IVU data route) keeps the lists of the trips
    /// after it on their own trips.
    #[test]
    fn stop_lists_belong_to_the_trip_before_them() {
        let text = "[infosystem_trip]\r\n45581\r\nZOB-HOHENECK\r\n81\r\n455\r\n\r\n\
            [infosystem_busstop_list]\r\n2\r\nZOB\r\nHoheneck\r\n\r\n\
            [infosystem_trip]\r\n455900\r\nIVU\r\n81\r\n455\r\n\r\n\
            [infosystem_trip]\r\n45503\r\nHBF-BERGERFUERTH\r\n3\r\n455\r\n\r\n\
            [infosystem_busstop_list]\r\n3\r\nHauptbahnhof\r\nMarkt\r\nBergerfuerth\r\n";
        let h = Hof::parse(&CfgFile::from_str("test.hof", text));
        assert_eq!(h.info_trips.len(), 3);
        assert_eq!(h.info_busstop_lists.len(), 3);
        assert_eq!(h.info_busstop_lists[0], vec!["ZOB", "Hoheneck"]);
        assert!(h.info_busstop_lists[1].is_empty());
        assert_eq!(
            h.info_busstop_lists[2],
            vec!["Hauptbahnhof", "Markt", "Bergerfuerth"]
        );
    }
}
