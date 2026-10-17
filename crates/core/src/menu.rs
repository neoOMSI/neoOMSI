use crate::ui::{StartPage, VehRow};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct TourItem {
    pub number: String,
    pub span: String,
    pub start: f32,
    pub trips: Vec<String>,
    pub runs: Vec<(f32, String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct LineItem {
    pub name: String,
    pub title: String,
    pub tours: Vec<TourItem>,
}

fn hm(m: f32) -> String {
    let t = m.max(0.0) as i32;
    format!("{:02}:{:02}", t / 60 % 24, t % 60)
}

fn run_time(m: f32) -> String {
    let t = m.max(0.0) as i32;
    format!("{:02}:{:02}", t / 60, t % 60)
}

fn first_stop(tt: &::timetable::TimetableData, trip: &::timetable::Trip) -> String {
    let named = |id: i64| tt.bus_stops.iter().find(|b| b.object_id == id).map(|b| b.name.trim().to_string());
    match trip.stations.first() {
        Some(id) => named(*id).unwrap_or_default(),
        None => trip.stations_legacy.first().and_then(|r| r.get(2)).map(|n| n.trim().to_string()).unwrap_or_default(),
    }
}

fn now_local() -> (i32, i32, i32, i32, i32) {
    use chrono::{Datelike, Timelike};
    let n = chrono::Local::now();
    (n.year(), n.month() as i32, n.day() as i32, n.hour() as i32, n.minute() as i32)
}

/// The dropdown row of the item `is` finds in `items`: row 0 is "Auto", the items follow.
fn auto_row<T>(items: &[T], is: impl Fn(&T) -> bool) -> usize {
    items.iter().position(is).map_or(0, |i| i + 1)
}

fn icao_code(text: &str) -> Option<String> {
    let code = text.trim().to_ascii_uppercase();
    (code.len() == 4 && code.chars().all(|c| c.is_ascii_alphanumeric())).then_some(code)
}

pub fn days_in_year(y: i32) -> i32 {
    if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
        366
    } else {
        365
    }
}

pub fn days_in_month(y: i32, m: i32) -> i32 {
    match m {
        2 => days_in_year(y) - 337,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// 0 = Monday
pub fn weekday(y: i32, m: i32, d: i32) -> i32 {
    let yy = if m <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    (days + 3).rem_euclid(7)
}

const WEATHER_MODES: usize = 4;

#[derive(Clone, Debug)]
pub enum Probe {
    Checking,
    Online(::network::ws::ServerInfo),
    Offline(String),
}

pub type Probes = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, Probe)>>>;

pub struct Menu {
    pub probes: Probes,
    pub join_pending: bool,
    pub drv_open: bool,
    pub drv_names: Vec<String>,
    pub drv_new: Option<String>,
    pub drv_confirm: Option<usize>,
    pub drv_note: String,
    pub maps: Vec<(String, String)>,
    pub map_entries: Vec<Vec<(u32, String)>>,
    pub vehicles: Vec<(String, String)>,
    pub vehicle_meta: Vec<(String, String)>,
    pub map: usize,
    pub vehicle: usize,
    pub hour: i32,
    pub minute: i32,
    pub year: i32,
    pub season_mode: usize,
    pub cal_open: bool,
    pub cal_year: i32,
    pub cal_month: i32,
    pub time_buf: String,
    pub weather_top: usize,
    pub icao: String,
    pub metar_live: bool,
    pub slider: Option<usize>,
    pub on_foot: bool,
    pub icao_check: Option<(String, Option<String>)>,
    pub icao_job: Option<(String, std::sync::mpsc::Receiver<Option<String>>)>,
    pub icao_all: Vec<(String, String)>,
    pub icao_all_job: Option<std::sync::mpsc::Receiver<Vec<(String, String)>>>,
    pub icao_all_started: bool,
    pub icao_top: usize,
    pub custom: crate::weather_setup::CustomWeather,
    pub traffic: usize,
    pub passengers: bool,
    pub schedule: bool,
    pub row: usize,
    pub page: StartPage,
    pub sel: usize,
    pub zone: usize,
    pub map_top: usize,
    pub address: String,
    pub addr_sel: bool,
    pub addr_focus: bool,
    pub mp_tab: usize,
    pub host: bool,
    pub servers: Vec<(String, String)>,
    pub server_top: usize,
    pub continue_sit: Option<String>,
    pub clicked: Option<(std::time::Instant, usize)>,
    pub situations: Vec<(String, String)>,
    pub situation: usize,
    pub weathers: Vec<(String, String)>,
    pub weather: usize,
    pub day: i32,
    pub sect: usize,
    pub focus: usize,
    pub veh_top: usize,
    pub veh_query: String,
    pub veh_open: Option<String>,
    pub drop: usize,
    pub drop_top: usize,
    drop_typed: String,
    drop_typed_at: Option<std::time::Instant>,
    pub edit: u8,
    pub details: bool,
    pub liveries: Vec<String>,
    pub livery: usize,
    pub depots: Vec<(String, String)>,
    pub depot: usize,
    pub numbers: Vec<String>,
    pub number: usize,
    pub plate: String,
    pub opts_for: Option<usize>,
    pub route: bool,
    pub lines: Vec<LineItem>,
    pub lines_for: Option<String>,
    pub line: usize,
    pub tour: usize,
    pub trip: usize,
    pub want_route: Option<(String, String, String, String)>,
    pub want_bus: Option<(String, String, String, String, String)>,
    pub line_top: usize,
    pub tour_top: usize,
    pub line_query: String,
    pub tour_query: String,
    pub entry: Option<u32>,
    pub entry_for: String,
    pub entry_top: usize,
    pub pic_key: String,
    pub route_tex: Option<(::render::TextureId, u32, u32)>,
    pub zoom: f32,
    pub pan: (f32, f32),
    pub view_for: String,
    pub drag: Option<((f32, f32), (f32, f32))>,
    pub mini: Option<(String, crate::start_menu::minimap::Minimap)>,
    pub mini_job: Option<(String, std::sync::mpsc::Receiver<Option<crate::start_menu::minimap::Minimap>>)>,
    pub mini_want: Option<(String, std::time::Instant)>,
    pub mini_failed: Option<String>,
    pub update: crate::updater::Updater,
    pub content_generation: u64,
}

fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let chunks = |s: &str| -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.to_lowercase().chars() {
            let d = c.is_ascii_digit();
            match out.last_mut() {
                Some((k, t)) if *k == d => t.push(c),
                _ => out.push((d, c.to_string())),
            }
        }
        out
    };
    let (x, y) = (chunks(a), chunks(b));
    for (p, q) in x.iter().zip(y.iter()) {
        let o = if p.0 && q.0 {
            let (m, n) = (p.1.trim_start_matches('0'), q.1.trim_start_matches('0'));
            m.len().cmp(&n.len()).then_with(|| m.cmp(n))
        } else {
            p.1.cmp(&q.1)
        };
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    x.len().cmp(&y.len())
}

impl Menu {
    pub fn probe(&self, addr: &str, every: f32) {
        let key = addr.trim().to_string();
        if key.is_empty() {
            return;
        }
        {
            let Ok(mut g) = self.probes.lock() else {
                return;
            };
            if let Some((t, p)) = g.get(&key) {
                if matches!(p, Probe::Checking) || t.elapsed().as_secs_f32() < every {
                    return;
                }
            }
            g.insert(key.clone(), (std::time::Instant::now(), Probe::Checking));
        }
        let map = self.probes.clone();
        std::thread::spawn(move || {
            let p = match ::network::ws::query(&key, false) {
                Ok(i) => Probe::Online(i),
                Err(e) => Probe::Offline(e),
            };
            if let Ok(mut g) = map.lock() {
                g.insert(key, (std::time::Instant::now(), p));
            }
        });
    }

    pub fn probe_of(&self, addr: &str) -> Option<Probe> {
        self.probes.lock().ok()?.get(addr.trim()).map(|x| x.1.clone())
    }

    pub fn new(root: &Path, default_map: &str) -> Menu {
        let content_generation = ::legacy_config::content_generation();
        let merged = |rel: &str| {
            let mut dirs = ::mods::read_dir_merged(rel);
            if dirs.is_empty() {
                dirs = ::legacy_config::vfs::read_dir_paths(&root.join(rel));
            }
            dirs.retain(|d| ::legacy_config::vfs::is_dir(d));
            dirs.sort_by_key(|d| d.file_name().map(|n| n.to_ascii_lowercase()));
            dirs
        };
        let mut maps = Vec::new();
        let mut map_entries: Vec<Vec<(u32, String)>> = Vec::new();
        for d in merged("maps") {
            let cfg = ::mods::resolve_path(&d, "global.cfg");
            if !::legacy_config::vfs::is_file(&cfg) {
                continue;
            }
            let g = ::map::GlobalCfg::load(&cfg).ok();
            let name = g
                .as_ref()
                .map(|g| g.name.clone())
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| d.file_name().unwrap().to_string_lossy().into_owned());
            map_entries.push(
                g.as_ref()
                    .map(|g| {
                        g.entry_point_groups()
                            .into_iter()
                            .map(|(n, places)| (places[0] as u32, n.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
            );
            maps.push((
                name,
                format!(
                    "maps/{}/global.cfg",
                    d.file_name().unwrap().to_string_lossy()
                ),
            ));
        }
        let mut vehicles = Vec::new();
        let mut vehicle_meta = Vec::new();
        for d in merged("Vehicles") {
            let folder = d.file_name().unwrap().to_string_lossy().to_string();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut files: Vec<PathBuf> = Vec::new();
            for dd in ::mods::mirrored_dirs(&d) {
                for f in ::legacy_config::vfs::read_dir_paths(&dd) {
                    if seen.insert(
                        f.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .to_ascii_lowercase(),
                    ) {
                        files.push(f);
                    }
                }
            }
            files.sort_by_key(|f| {
                f.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_ascii_lowercase()
            });
            for (f, v) in ::legacy_vehicle::vehicle::offered_vehicles(&files) {
                let rel = format!(
                    "Vehicles/{}/{}",
                    folder,
                    f.file_name().unwrap().to_string_lossy()
                );
                let stem = f.file_stem().unwrap().to_string_lossy().to_string();
                let name = Some(
                    format!("{} {}", v.manufacturer, v.type_name)
                        .trim()
                        .to_string(),
                )
                    .filter(|n| !n.is_empty())
                    .unwrap_or(stem);
                vehicle_meta.push((
                    v.manufacturer.trim().to_string(),
                    Some(v.type_name.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| name.clone()),
                ));
                vehicles.push((name, rel));
            }
        }
        log::info!("menu: {} vehicles offered", vehicles.len());
        let mut situations = vec![("-".to_string(), String::new())];
        let mut files: Vec<PathBuf> = ::mods::read_dir_merged("Situations");
        if files.is_empty() {
            files = std::fs::read_dir(root.join("Situations"))
                .map(|rd| rd.flatten().map(|e| e.path()).collect())
                .unwrap_or_default();
        }
        files.retain(|p| {
            p.extension()
                .map(|e| e.eq_ignore_ascii_case("osn"))
                .unwrap_or(false)
        });
        files.sort_by_key(|p| p.file_name().map(|n| n.to_ascii_lowercase()));
        {
            for f in files {
                let name = ::content::Situation::load(&f)
                    .map(|s| s.name)
                    .ok()
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| f.file_stem().unwrap().to_string_lossy().into_owned());
                situations.push((
                    name,
                    format!("Situations/{}", f.file_name().unwrap().to_string_lossy()),
                ));
            }
        }
        let mut weathers = vec![
            ("default".to_string(), String::new()),
            ("cycle".to_string(), crate::weather_cycle::CYCLE.to_string()),
            ("real".to_string(), "metar-sync:".to_string()),
            ("custom".to_string(), "custom".to_string()),
        ];
        if let Ok(rd) = std::fs::read_dir(root.join("Weather")) {
            let mut files: Vec<PathBuf> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .map(|e| e.eq_ignore_ascii_case("owt"))
                        .unwrap_or(false)
                })
                .collect();
            files.sort();
            for f in files {
                let name = f
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .trim_start_matches('#')
                    .to_string();
                weathers.push((
                    name,
                    format!("Weather/{}", f.file_name().unwrap().to_string_lossy()),
                ));
            }
        }
        let map = maps
            .iter()
            .position(|m| m.1.eq_ignore_ascii_case(default_map))
            .unwrap_or(0);
        let vehicle = vehicles
            .iter()
            .position(|v| v.1.to_ascii_lowercase().contains("sd80"))
            .unwrap_or(0);
        let mut m = Menu {
            maps,
            map_entries,
            vehicles,
            vehicle_meta,
            map,
            vehicle,
            hour: 9,
            minute: 0,
            year: now_local().0,
            season_mode: 0,
            cal_open: false,
            cal_year: now_local().0,
            cal_month: 0,
            time_buf: String::new(),
            weather_top: 0,
            icao: String::new(),
            metar_live: true,
            slider: None,
            on_foot: ::config::get_bool("gameplay", "start_on_foot").unwrap_or(false),
            icao_check: None,
            icao_job: None,
            icao_all: Vec::new(),
            icao_all_job: None,
            icao_all_started: false,
            icao_top: 0,
            custom: Default::default(),
            traffic: 30,
            passengers: true,
            schedule: true,
            row: 0,
            page: StartPage::Start,
            sel: 0,
            zone: 0,
            map_top: 0,
            address: String::new(),
            addr_sel: false,
            addr_focus: false,
            probes: Default::default(),
            join_pending: false,
            drv_open: false,
            drv_names: Vec::new(),
            drv_new: None,
            drv_confirm: None,
            drv_note: String::new(),
            mp_tab: 0,
            host: ::config::get_bool("gameplay", "host_next_drive").unwrap_or(false),
            servers: omsi_launcher_lib::servers::load().into_iter().map(|s| (s.name, s.address)).collect(),
            server_top: 0,
            continue_sit: None,
            clicked: None,
            situations,
            situation: 0,
            weathers,
            weather: 0,
            day: 150,
            sect: 0,
            focus: 0,
            veh_top: 0,
            veh_query: String::new(),
            veh_open: None,
            drop: 0,
            drop_top: 0,
            drop_typed: String::new(),
            drop_typed_at: None,
            edit: 0,
            details: false,
            liveries: Vec::new(),
            livery: 0,
            depots: Vec::new(),
            depot: 0,
            numbers: Vec::new(),
            number: 0,
            plate: String::new(),
            opts_for: None,
            route: false,
            lines: Vec::new(),
            lines_for: None,
            line: 0,
            tour: 0,
            trip: 0,
            want_route: None,
            want_bus: None,
            line_top: 0,
            tour_top: 0,
            line_query: String::new(),
            tour_query: String::new(),
            entry: None,
            entry_for: String::new(),
            entry_top: 0,
            pic_key: String::new(),
            route_tex: None,
            zoom: 1.0,
            pan: (0.0, 0.0),
            view_for: String::new(),
            drag: None,
            mini: None,
            mini_job: None,
            mini_want: None,
            mini_failed: None,
            update: Default::default(),
            content_generation,
        };
        m.veh_open = m.maker_key(m.vehicle);
        m
    }

    /// Reads the lists again if mods changed since, keeping each pick that is still there.
    pub fn reload_content(&mut self, root: &Path) {
        if self.content_generation == ::legacy_config::content_generation() {
            return;
        }
        // (an install that finished outside the Mods page is not mounted yet)
        let _ = omsi_launcher_lib::register_content();
        let path_at = |list: &[(String, String)], index: usize| {
            list.get(index).map(|(_, path)| path.clone()).unwrap_or_default()
        };
        let index_of = |list: &[(String, String)], wanted: &str, fallback: usize| {
            list.iter().position(|(_, path)| path.eq_ignore_ascii_case(wanted)).unwrap_or(fallback)
        };
        let map_path = path_at(&self.maps, self.map);
        let vehicle_path = path_at(&self.vehicles, self.vehicle);
        let situation_path = path_at(&self.situations, self.situation);
        let weather_path = path_at(&self.weathers, self.weather);
        let fresh = Menu::new(root, &map_path);
        self.map = fresh.map;
        self.vehicle = index_of(&fresh.vehicles, &vehicle_path, fresh.vehicle);
        self.situation = index_of(&fresh.situations, &situation_path, 0);
        self.weather = index_of(&fresh.weathers, &weather_path, 0);
        if fresh.vehicles.get(self.vehicle).is_none_or(|(_, path)| *path != vehicle_path) {
            self.opts_for = None;
        }
        (self.map_top, self.veh_top) = (0, 0);
        self.maps = fresh.maps;
        self.map_entries = fresh.map_entries;
        self.vehicles = fresh.vehicles;
        self.vehicle_meta = fresh.vehicle_meta;
        self.situations = fresh.situations;
        self.weathers = fresh.weathers;
        self.content_generation = fresh.content_generation;
        log::info!("menu: content changed, lists read again");
    }

    fn maker_key(&self, i: usize) -> Option<String> {
        self.vehicle_meta.get(i).map(|x| x.0.trim().to_lowercase())
    }

    pub fn veh_groups(&self) -> Vec<(String, String, Vec<usize>)> {
        let q = self.veh_query.trim().to_lowercase();
        let unknown = ::i18n::translate("start.maps.unknown", &[]);
        let mut groups: Vec<(String, String, Vec<usize>)> = Vec::new();
        for (i, v) in self.vehicles.iter().enumerate() {
            let (maker, ty) = self.vehicle_meta.get(i).cloned().unwrap_or_default();
            let hay = format!("{} {} {}", maker, ty, v.0).to_lowercase();
            if !q.is_empty() && !hay.contains(&q) {
                continue;
            }
            let key = maker.trim().to_lowercase();
            match groups.iter_mut().find(|g| g.0 == key) {
                Some(g) => g.2.push(i),
                None => groups.push((
                    key,
                    if maker.trim().is_empty() { unknown.clone() } else { maker.trim().to_string() },
                    vec![i],
                )),
            }
        }
        for g in groups.iter_mut() {
            g.2.sort_by(|a, b| {
                let (x, y) = (&self.vehicle_meta[*a].1, &self.vehicle_meta[*b].1);
                natural_cmp(x, y).then_with(|| a.cmp(b))
            });
        }
        groups.sort_by(|a, b| natural_cmp(&a.1, &b.1));
        groups
    }

    pub fn veh_rows(&self) -> Vec<VehRow> {
        let searching = !self.veh_query.trim().is_empty();
        let mut rows = Vec::new();
        for (key, name, items) in self.veh_groups() {
            let open = searching || self.veh_open.as_deref() == Some(key.as_str());
            rows.push(VehRow { label: name, count: items.len(), open, vehicle: None, on: false });
            if open {
                for i in items {
                    rows.push(VehRow {
                        label: self.vehicle_meta.get(i).map(|x| x.1.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| self.vehicles[i].0.clone()),
                        count: 0,
                        open: false,
                        vehicle: Some(i),
                        on: i == self.vehicle,
                    });
                }
            }
        }
        rows
    }

    pub fn veh_row_of_selected(&self) -> usize {
        let rows = self.veh_rows();
        rows.iter().position(|r| r.vehicle == Some(self.vehicle)).unwrap_or(0)
    }

    pub fn veh_pick(&mut self, row: usize) {
        let rows = self.veh_rows();
        let Some(r) = rows.get(row) else {
            return;
        };
        match r.vehicle {
            Some(i) => self.vehicle = i,
            None => {
                let key = self.veh_groups().get(rows[..=row].iter().filter(|x| x.vehicle.is_none()).count() - 1).map(|g| g.0.clone());
                self.veh_open = if self.veh_open == key { None } else { key };
            }
        }
    }

    pub fn refresh_options(&mut self, root: &Path) {
        if self.opts_for == Some(self.vehicle) {
            return;
        }
        self.opts_for = Some(self.vehicle);
        self.liveries.clear();
        self.depots.clear();
        self.numbers.clear();
        self.livery = 0;
        self.depot = 0;
        self.number = 0;
        self.plate.clear();
        let Some(bus) = self.vehicles.get(self.vehicle).map(|v| v.1.clone()) else {
            return;
        };
        let Ok(path) = crate::spawn::player_bus_path(root, &bus) else {
            return;
        };
        let Ok(def) = ::legacy_vehicle::Vehicle::load(&path) else {
            return;
        };
        if let Some(m) = def.model.as_ref() {
            if let Ok(model) = ::model::Model::load(&::mods::resolve_path(def.dir(), m)) {
                let mut names: Vec<String> = model
                    .ctc
                    .iter()
                    .flat_map(|c| ::simulation::vehicle::load_paint_schemes(&::mods::resolve_path(def.dir(), &c.path)))
                    .map(|s| s.name)
                    .collect();
                names.sort_by_key(|n| n.to_lowercase());
                names.dedup();
                self.liveries = names;
            }
        }
        let mut files: Vec<(String, String)> = ::legacy_vehicle::hof::depot_files(def.dir())
            .into_iter()
            .map(|f| {
                let file = f.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                let label = ::legacy_vehicle::Hof::load(&f)
                    .ok()
                    .map(|h| h.name.trim().to_string())
                    .filter(|n| !n.is_empty() && !file.to_ascii_lowercase().starts_with(&n.to_ascii_lowercase()))
                    .map(|n| format!("{n}  ({file})"))
                    .unwrap_or_else(|| file.clone());
                (label, file)
            })
            .collect();
        files.sort_by_key(|(l, _)| l.to_lowercase());
        self.depots = files;
        let mut numbers: Vec<String> = def.numbers_with_plates().into_iter().map(|(n, _)| n).collect();
        numbers.sort_by(|a, b| natural_cmp(a, b));
        numbers.dedup();
        self.numbers = numbers;
        if self.want_bus.as_ref().is_some_and(|w| w.0.eq_ignore_ascii_case(&bus)) {
            let (_, livery, depot, number, plate) = self.want_bus.take().unwrap_or_default();
            self.livery = auto_row(&self.liveries, |l| *l == livery);
            self.depot = auto_row(&self.depots, |d| d.1.eq_ignore_ascii_case(&depot));
            self.number = auto_row(&self.numbers, |n| *n == number);
            self.plate = plate;
        }
    }

    pub fn recall(&mut self) {
        let get = |k: &str| ::config::get_string("start_menu", k).unwrap_or_default();
        self.route = !::config::get_bool("start_menu", "free_drive").unwrap_or(true);
        if !get("line").is_empty() {
            self.want_route = Some((get("map"), get("line"), get("tour"), get("trip")));
        }
        if let Some(i) = self.vehicles.iter().position(|v| v.1.eq_ignore_ascii_case(&get("bus"))) {
            self.vehicle = i;
            self.veh_open = self.maker_key(i);
            self.want_bus = Some((get("bus"), get("livery"), get("depot"), get("fleet_number"), get("plate")));
        }
        if let Some(w) = ::config::get_string("start_menu", "weather") {
            if let Some(i) = self.weathers.iter().position(|x| x.1.eq_ignore_ascii_case(&w)) {
                self.weather = i;
            }
        }
        self.metar_live = ::config::get_bool("start_menu", "metar_live").unwrap_or(true);
        self.icao = get("icao");
        if let Some(c) = crate::weather_setup::custom_weather(Some(&get("custom_weather"))) {
            self.custom = c;
        }
    }

    pub fn remember(&self) {
        let set = |k: &str, v: String| ::config::set_setting("start_menu", k, v);
        ::config::set_setting("start_menu", "free_drive", !self.route);
        if let Some((l, t)) = self.chosen_tour() {
            set("map", self.maps.get(self.map).map(|m| m.1.clone()).unwrap_or_default());
            set("line", l.name.clone());
            set("tour", t.number.clone());
            set("trip", self.trip_arg().unwrap_or_default());
        }
        if let Some(bus) = self.vehicles.get(self.vehicle).filter(|_| self.opts_for == Some(self.vehicle)) {
            set("bus", bus.1.clone());
            set("livery", self.livery.checked_sub(1).and_then(|i| self.liveries.get(i)).cloned().unwrap_or_default());
            set("depot", self.depot.checked_sub(1).and_then(|i| self.depots.get(i)).map(|d| d.1.clone()).unwrap_or_default());
            set("fleet_number", self.number.checked_sub(1).and_then(|i| self.numbers.get(i)).cloned().unwrap_or_default());
            set("plate", self.plate.trim().to_string());
        }
        set("weather", self.weathers.get(self.weather).map(|w| w.1.clone()).unwrap_or_default());
        ::config::set_setting("start_menu", "metar_live", self.metar_live);
        set("icao", self.icao.trim().to_string());
        set("custom_weather", self.custom.encode());
        if let Err(e) = ::config::save() {
            log::warn!("settings: cannot save: {e}");
        }
    }

    pub fn livery_text(&self) -> String {
        match self.livery {
            0 => ::i18n::translate("start.maps.auto", &[]),
            i => self.liveries.get(i - 1).cloned().unwrap_or_default(),
        }
    }

    pub fn depot_text(&self) -> String {
        match self.depot {
            0 => ::i18n::translate("start.maps.auto", &[]),
            i => self.depots.get(i - 1).map(|d| d.0.clone()).unwrap_or_default(),
        }
    }

    pub fn number_text(&self) -> String {
        match self.number {
            0 => ::i18n::translate("start.maps.auto", &[]),
            i => self.numbers.get(i - 1).cloned().unwrap_or_default(),
        }
    }

    pub fn drop_dim(&self, kind: usize) -> Vec<bool> {
        let now = (self.hour * 60 + self.minute) as f32;
        match (kind, self.chosen_tour()) {
            (16, Some((_, t))) => t.runs.iter().map(|r| r.0 < now).collect(),
            _ => Vec::new(),
        }
    }

    pub fn drop_items(&self, kind: usize) -> Vec<String> {
        if kind == 11 {
            return self.map_names();
        }
        if kind == 12 {
            return self.entry_rows().into_iter().map(|r| r.1).collect();
        }
        if kind == 13 {
            return crate::weather_setup::CUSTOM_CLOUDS.iter().map(|s| s.to_string()).collect();
        }
        if kind == 14 {
            return crate::weather_setup::CUSTOM_PRECIP.iter().map(|s| s.to_string()).collect();
        }
        if kind == 16 {
            return self.trip_rows();
        }
        let mut v = vec![::i18n::translate("start.maps.auto", &[])];
        match kind {
            8 => v.extend(self.liveries.iter().cloned()),
            9 => v.extend(self.depots.iter().map(|d| d.0.clone())),
            _ => v.extend(self.numbers.iter().cloned()),
        }
        v
    }

    pub fn drop_sel(&self, kind: usize) -> usize {
        match kind {
            11 => self.map,
            13 => self.custom.cloud,
            14 => self.custom.precip.clamp(0, 2) as usize,
            12 => {
                let shown = self.entry_shown();
                self.entry_rows().iter().position(|r| r.0 == shown).unwrap_or(0)
            }
            8 => self.livery,
            9 => self.depot,
            16 => self.trip,
            _ => self.number,
        }
    }

    pub fn drop_pick(&mut self, kind: usize, i: usize) {
        match kind {
            11 => self.map = i,
            13 => self.custom_set(1, i as f32 / (crate::weather_setup::CUSTOM_CLOUDS.len() - 1).max(1) as f32),
            14 => self.custom_set(2, i as f32 / 2.0),
            12 => {
                if let Some(r) = self.entry_rows().get(i) {
                    self.entry_pick(r.0);
                }
            }
            8 => self.livery = i,
            9 => self.depot = i,
            16 => self.pick_trip(i),
            _ => self.number = i,
        }
        self.drop = 0;
    }

    /// The same letter typed again cycles through the entries starting with it.
    pub fn drop_type(&mut self, text: &str) {
        let kind = self.drop;
        let typed: String = text.chars().filter(|c| !c.is_control()).flat_map(char::to_lowercase).collect();
        if kind == 0 || typed.is_empty() {
            return;
        }
        let now = std::time::Instant::now();
        if self.drop_typed_at.is_none_or(|t| now.duration_since(t).as_millis() > 900) {
            self.drop_typed.clear();
        }
        self.drop_typed_at = Some(now);
        self.drop_typed.push_str(&typed);
        let items = self.drop_items(kind);
        let n = items.len();
        if n == 0 {
            return;
        }
        let mut chars = self.drop_typed.chars();
        let first = chars.next();
        let (needle, from) = if chars.all(|c| Some(c) == first) {
            (first.map(String::from).unwrap_or_default(), self.drop_sel(kind) + 1)
        } else {
            (self.drop_typed.clone(), self.drop_sel(kind))
        };
        let hit = (0..n)
            .map(|k| (from + k) % n)
            .find(|&i| items[i].trim().to_lowercase().starts_with(&needle));
        if let Some(i) = hit {
            self.drop_pick(kind, i);
            self.drop = kind;
            self.drop_top = i.saturating_sub(2);
        }
    }

    pub fn drop_step(&mut self, d: i32) {
        let kind = self.drop;
        let n = self.drop_items(kind).len() as i32;
        if kind == 0 || n == 0 {
            return;
        }
        let i = (self.drop_sel(kind) as i32 + d).clamp(0, n - 1) as usize;
        self.drop_pick(kind, i);
        self.drop = kind;
        if i < self.drop_top {
            self.drop_top = i;
        } else if i >= self.drop_top + 6 {
            self.drop_top = i - 5;
        }
    }

    pub fn drop_toggle(&mut self, kind: usize) {
        self.drop = if self.drop == kind { 0 } else { kind };
        self.drop_top = self.drop_sel(kind).saturating_sub(2);
    }

    pub fn refresh_continue(&mut self) {
        self.continue_sit = self
            .maps
            .iter()
            .flat_map(|m| ::omsi_launcher_lib::saved_situations(&m.1))
            .max_by_key(|s| s.saved)
            .map(|s| s.file.to_string_lossy().into_owned());
    }

    pub fn apply_last_situation(&mut self) {
        let Some(file) = self.continue_sit.clone() else {
            return;
        };
        let Ok(sit) = ::content::Situation::load(Path::new(&file)) else {
            return;
        };
        let tail = |path: &str, folder: &str| -> String {
            let p = path.replace('\\', "/").to_ascii_lowercase();
            match p.rfind(&format!("{folder}/")) {
                Some(i) => p[i..].to_string(),
                None => p,
            }
        };
        let want = tail(&sit.map, "maps");
        if let Some(i) = self.maps.iter().position(|m| tail(&m.1, "maps") == want) {
            self.map = i;
        }
        let mine = sit
            .vehicles
            .iter()
            .find(|v| v.is_my_vehicle)
            .or_else(|| sit.vehicles.get(sit.my_vehicle.max(0) as usize));
        if let Some(v) = mine {
            let want = tail(&v.file, "vehicles");
            if let Some(i) = self.vehicles.iter().position(|x| tail(&x.1, "vehicles") == want) {
                self.vehicle = i;
            }
        }
        let (y, doy, h, m, _) = sit.time;
        self.year = y.clamp(1970, 2100);
        self.day = doy.clamp(1, days_in_year(self.year));
        self.hour = h.clamp(0, 23);
        self.minute = m.clamp(0, 59);
        self.cal_year = self.year;
        self.cal_month = self.ymd().1 - 1;
        self.season_mode = 0;
        self.weather = match &sit.weather {
            Some(w) => self
                .weathers
                .iter()
                .position(|x| !x.1.is_empty() && x.1.eq_ignore_ascii_case(w))
                .unwrap_or(0),
            None => 0,
        };
    }

    pub fn map_names(&self) -> Vec<String> {
        self.maps.iter().map(|m| m.0.clone()).collect()
    }

    pub fn rows(&self) -> Vec<(String, String)> {
        let tr = |k: &str| ::i18n::translate(k, &[]);
        let onoff = |b: bool| tr(if b { "start.value.on" } else { "start.value.off" });
        let pick = |v: Option<&(String, String)>| v.map(|x| x.0.clone()).unwrap_or_else(|| "-".to_string());
        vec![
            (tr("start.row.vehicle"), pick(self.vehicles.get(self.vehicle))),
            (
                tr("start.row.time"),
                self.time_text(),
            ),
            (
                tr("start.row.traffic"),
                ::i18n::translate("start.value.vehicles", &[("n", &self.traffic)]),
            ),
            (tr("start.row.passengers"), onoff(self.passengers)),
            (tr("start.row.timetable"), onoff(self.schedule)),
            (
                tr("start.row.weather"),
                self.weather_label(self.weather),
            ),
            (
                tr("start.row.date"),
                self.date_text(),
            ),
            (tr("start.row.situation"), self.route_text()),
            (tr("start.row.season"), self.season_text()),
        ]
    }

    pub fn route_text(&self) -> String {
        match self.chosen_tour() {
            Some((l, t)) if !self.trip_text().is_empty() => format!("{}  -  {}  -  {}", l.name, t.number, self.trip_text()),
            Some((l, t)) => format!("{}  -  {}", l.name, t.number),
            None => "-".to_string(),
        }
    }

    pub fn chosen_tour(&self) -> Option<(&LineItem, &TourItem)> {
        let l = self.lines.get(self.line)?;
        Some((l, l.tours.get(self.tour)?))
    }

    pub fn refresh_lines(&mut self, root: &Path) {
        let Some(map) = self.maps.get(self.map).map(|m| m.1.clone()) else {
            return;
        };
        let (y, mo, d) = self.ymd();
        let date = y * 10000 + mo * 100 + d;
        let key = format!("{map}|{date}");
        if self.lines_for.as_deref() == Some(key.as_str()) {
            return;
        }
        let same_map = self.lines_for.as_deref().and_then(|k| k.rsplit_once('|')).is_some_and(|k| k.0 == map);
        if !same_map {
            self.line_query.clear();
            self.tour_query.clear();
        }
        let keep = self.lines.get(self.line).map(|l| l.name.clone()).filter(|_| same_map);
        let keep_tour = self.chosen_tour().map(|(_, t)| t.number.clone()).filter(|_| same_map);
        self.lines_for = Some(key);
        self.lines.clear();
        self.line = 0;
        self.tour = 0;
        self.line_top = 0;
        self.tour_top = 0;
        let cfg = ::mods::resolve_path(root, &map);
        let Some(dir) = cfg.parent() else {
            return;
        };
        let chrono = ::map::active_chrono_dirs(dir, date);
        let off = ::map::chrono_deactivated_lines(&chrono);
        let tt = ::timetable::TimetableData::load_with_chrono(dir, &chrono, &off);
        let calendar = ::map::Calendar::load(&dir.join("Holidays.txt")).unwrap_or_default();
        let clock = ::simulation::SimClock { year: self.year, day_of_year: self.day, ..Default::default() };
        let bits = crate::schedule::day_bits(&calendar, &clock);
        let next_day_bit = 1 << ((clock.weekday() + 1) % 7);
        let runs_today = |t: &&::timetable::Tour| crate::schedule::tour_runs(t, bits, next_day_bit);
        let mut chosen: Vec<&::timetable::Line> =
            tt.lines.iter().filter(|l| l.user_allowed && l.tours.iter().any(|t| runs_today(&t))).collect();
        if chosen.is_empty() {
            chosen = tt.lines.iter().filter(|l| l.tours.iter().any(|t| runs_today(&t))).collect();
        }
        let mut lines: Vec<LineItem> = chosen
            .into_iter()
            .map(|l| {
                let title = l
                    .tours
                    .first()
                    .and_then(|t| t.trips.first())
                    .and_then(|t| tt.trip(&t.trip))
                    .map(|t| t.terminus.trim().to_string())
                    .unwrap_or_default();
                let mut tours: Vec<TourItem> = l
                    .tours
                    .iter()
                    .filter(runs_today)
                    .map(|t| {
                        let first = t.trips.iter().map(|x| x.departure).fold(f32::MAX, f32::min);
                        let last = t.trips.iter().map(|x| x.departure).fold(0.0, f32::max);
                        let mut trips: Vec<String> = Vec::new();
                        for x in &t.trips {
                            if !trips.iter().any(|n| n.eq_ignore_ascii_case(&x.trip)) {
                                trips.push(x.trip.clone());
                            }
                        }
                        let mut runs: Vec<(f32, String, String)> = t
                            .trips
                            .iter()
                            .map(|x| {
                                let trip = tt.trip(&x.trip);
                                let to = trip.map(|t| t.terminus.trim().to_string()).unwrap_or_default();
                                let label = match trip.map(|t| first_stop(&tt, t)).filter(|f| !f.is_empty()) {
                                    Some(from) => format!("{from} > {to}"),
                                    None => to,
                                };
                                (x.departure, label, x.trip.clone())
                            })
                            .collect();
                        runs.sort_by(|a, b| a.0.total_cmp(&b.0));
                        TourItem {
                            number: t.number.trim().to_string(),
                            span: if t.trips.is_empty() { String::new() } else { format!("{} - {}", hm(first), hm(last)) },
                            start: if t.trips.is_empty() { 0.0 } else { first },
                            trips,
                            runs,
                        }
                    })
                    .collect();
                tours.sort_by(|a, b| natural_cmp(&a.number, &b.number).then_with(|| a.start.total_cmp(&b.start)));
                LineItem { name: l.name.trim().to_string(), title, tours }
            })
            .collect();
        lines.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        self.lines = lines;
        self.pick_line(0);
        if let Some(k) = keep {
            if let Some(l) = self.lines.iter().position(|x| x.name.eq_ignore_ascii_case(&k)) {
                self.pick_line(l);
                if let Some(t) = keep_tour.and_then(|n| self.lines[l].tours.iter().position(|x| x.number == n)) {
                    self.pick_tour(t);
                }
            }
        }
        self.fix_filters();
        if self.want_route.as_ref().is_some_and(|w| w.0.eq_ignore_ascii_case(&map)) {
            let (_, line, tour, trip) = self.want_route.take().unwrap_or_default();
            if let Some(l) = self.lines.iter().position(|x| x.name.eq_ignore_ascii_case(&line)) {
                self.pick_line(l);
                if let Some(t) = self.lines[l].tours.iter().position(|x| x.number.eq_ignore_ascii_case(&tour)) {
                    self.pick_tour(t);
                    self.trip = self.lines[l].tours[t].runs.iter().position(|r| run_time(r.0) == trip).unwrap_or(0);
                }
            }
        }
    }

    pub fn pick_tour(&mut self, i: usize) {
        if self.tour != i {
            self.trip = 0;
            self.entry = None;
        }
        self.tour = i;
    }

    pub fn trip_rows(&self) -> Vec<String> {
        let mut v = Vec::new();
        if let Some((_, t)) = self.chosen_tour() {
            v.extend(t.runs.iter().map(|(dep, to, _)| format!("{}  {to}", hm(*dep))));
        }
        v
    }

    pub fn trip_text(&self) -> String {
        self.trip_rows().get(self.trip).cloned().unwrap_or_default()
    }

    pub fn trip_arg(&self) -> Option<String> {
        let (_, t) = self.chosen_tour()?;
        t.runs.get(self.trip).map(|r| run_time(r.0))
    }

    fn pick_trip(&mut self, i: usize) {
        self.trip = i;
        let dep = self.chosen_tour().and_then(|(_, t)| t.runs.get(i)).map(|r| r.0);
        if let Some(dep) = dep {
            let at = (dep.floor() as i32 - 3).max(0);
            if self.hour * 60 + self.minute > at {
                self.hour = at / 60 % 24;
                self.minute = at % 60;
            }
        }
    }

    fn tour_left(&self, t: &TourItem) -> bool {
        let now = (self.hour * 60 + self.minute) as f32;
        t.runs.iter().any(|r| r.0 >= now)
    }

    pub fn line_rows(&self) -> Vec<(usize, String, String)> {
        let q = self.line_query.trim().to_lowercase();
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| q.is_empty() || format!("{} {}", l.name, l.title).to_lowercase().contains(&q) || l.tours.iter().any(|t| t.runs.iter().any(|r| r.1.to_lowercase().contains(&q))))
            .filter(|(_, l)| l.tours.iter().any(|t| self.tour_left(t)))
            .map(|(i, l)| (i, l.name.clone(), l.title.clone()))
            .collect()
    }

    pub fn tour_rows(&self) -> Vec<(usize, String, String)> {
        let q = self.tour_query.trim().to_lowercase();
        let Some(l) = self.lines.get(self.line) else {
            return Vec::new();
        };
        let word = ::i18n::translate("start.maps.tour", &[]);
        let word = {
            let w = word.to_lowercase();
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        };
        l.tours
            .iter()
            .enumerate()
            .filter(|(_, t)| q.is_empty() || format!("{} {}", t.number, t.span).to_lowercase().contains(&q))
            .filter(|(_, t)| self.tour_left(t))
            .map(|(i, t)| (i, format!("{word} {}", t.number), t.span.clone()))
            .collect()
    }

    pub fn pick_line(&mut self, i: usize) {
        self.line = i;
        self.entry = None;
        self.tour_query.clear();
        self.tour_top = 0;
        self.tour = 0;
        self.trip = 0;
    }

    pub fn line_step(&mut self, dir: i32) {
        let rows = self.line_rows();
        if rows.is_empty() {
            return;
        }
        let at = rows.iter().position(|r| r.0 == self.line).unwrap_or(0) as i32;
        let next = (at + dir).clamp(0, rows.len() as i32 - 1) as usize;
        if rows[next].0 != self.line {
            self.pick_line(rows[next].0);
        }
    }

    pub fn tour_step(&mut self, dir: i32) {
        let rows = self.tour_rows();
        if rows.is_empty() {
            return;
        }
        let at = rows.iter().position(|r| r.0 == self.tour).unwrap_or(0) as i32;
        self.pick_tour(rows[(at + dir).clamp(0, rows.len() as i32 - 1) as usize].0);
    }

    pub fn fix_filters(&mut self) {
        let rows = self.line_rows();
        if !rows.is_empty() && !rows.iter().any(|r| r.0 == self.line) {
            self.pick_line(rows[0].0);
        }
        let rows = self.tour_rows();
        if !rows.is_empty() && !rows.iter().any(|r| r.0 == self.tour) {
            self.pick_tour(rows[0].0);
        }
    }

    pub fn edit_buf(&mut self) -> (&mut String, usize) {
        match self.edit {
            1 => (&mut self.veh_query, 60),
            3 => (&mut self.line_query, 40),
            4 => (&mut self.tour_query, 40),
            5 => (&mut self.time_buf, 4),
            6 => (&mut self.icao, 4),
            _ => (&mut self.plate, 24),
        }
    }

    pub fn edit_changed(&mut self) {
        match self.edit {
            1 => self.veh_top = 0,
            3 => {
                self.line_top = 0;
                self.fix_filters();
            }
            4 => {
                self.tour_top = 0;
                self.fix_filters();
            }
            5 => self.apply_time(),
            6 => {
                self.icao = self.icao.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect();
                self.icao_top = 0;
            }
            _ => {}
        }
    }

    pub fn icao_matches(&self, _n: usize) -> Vec<(String, String)> {
        let q = self.icao.trim().to_ascii_uppercase();
        if !self.icao_all.is_empty() {
            let mut rows: Vec<&(String, String)> = self
                .icao_all
                .iter()
                .filter(|r| q.is_empty() || r.0.starts_with(&q) || r.1.to_ascii_uppercase().contains(&q))
                .collect();
            rows.sort_by_key(|r| !r.0.starts_with(&q));
            return rows.into_iter().cloned().collect();
        }
        match &self.icao_check {
            Some((c, Some(t))) if *c == self.icao.trim().to_ascii_uppercase() => vec![(c.clone(), format!("{c} - {t}"))],
            _ => match &self.icao_job {
                Some((c, _)) if *c == self.icao.trim().to_ascii_uppercase() => {
                    vec![(c.clone(), format!("{c} - {}", ::i18n::translate("start.maps.icao_checking", &[])))]
                }
                _ => Vec::new(),
            },
        }
    }

    pub fn want_icao(&mut self) {
        if !self.icao_all_started {
            self.icao_all_started = true;
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::weather_setup::metar_station_list());
            });
            self.icao_all_job = Some(rx);
        }
        let Some(code) = icao_code(&self.icao) else {
            return;
        };
        if self.icao_check.as_ref().is_some_and(|c| c.0 == code)
            || self.icao_job.as_ref().is_some_and(|(pending, _)| pending == &code)
        {
            return;
        }
        self.icao_job = None;
        let (tx, rx) = std::sync::mpsc::channel();
        let c = code.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::weather_setup::metar_station_info(&c).map(|i| i.name));
        });
        self.icao_job = Some((code, rx));
    }

    pub fn poll_icao(&mut self) {
        if let Some(rx) = self.icao_all_job.take() {
            match rx.try_recv() {
                Ok(list) => self.icao_all = list,
                Err(std::sync::mpsc::TryRecvError::Empty) => self.icao_all_job = Some(rx),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
            }
        }
        let Some((code, rx)) = self.icao_job.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(t) => self.icao_check = Some((code, t)),
            Err(std::sync::mpsc::TryRecvError::Empty) => self.icao_job = Some((code, rx)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.icao_check = Some((code, None)),
        }
    }

    pub fn icao_valid(&self) -> Option<String> {
        let code = icao_code(&self.icao)?;
        match &self.icao_check {
            Some((c, Some(_))) if *c == code => Some(code),
            _ if self.icao_all.iter().any(|r| r.0 == code) => Some(code),
            _ => None,
        }
    }

    pub fn icao_status(&self) -> String {
        let Some(code) = icao_code(&self.icao) else {
            return String::new();
        };
        match &self.icao_check {
            Some((checked, None)) if *checked == code => ::i18n::translate("start.maps.noair", &[]),
            _ => String::new(),
        }
    }

    pub fn mini_key(&self) -> Option<(String, String)> {
        let map = self.maps.get(self.map).map(|m| m.1.clone())?;
        let (y, mo, d) = self.ymd();
        Some((map, format!("{y}-{mo}-{d}")))
    }

    pub fn want_mini(&mut self) {
        let Some((name, date)) = self.mini_key() else {
            return;
        };
        let map = format!("{name}|{date}");
        if self.mini.as_ref().is_some_and(|m| m.0 == map) || self.mini_failed.as_deref() == Some(map.as_str()) {
            self.mini_want = None;
            return;
        }
        if !self.mini_want.as_ref().is_some_and(|w| w.0 == map) {
            self.mini_want = Some((map.clone(), std::time::Instant::now()));
        }
        if self.mini_job.is_some() {
            return;
        }
        if !self.mini_want.as_ref().is_some_and(|w| w.1.elapsed().as_millis() >= 400) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let r = crate::start_menu::minimap::minimap(&name, &date);
            if let Err(e) = &r {
                log::warn!("map preview of {name}: {e:#}");
            }
            let _ = tx.send(r.ok());
        });
        self.mini_job = Some((map, rx));
    }

    pub fn poll_mini(&mut self) {
        let Some((map, rx)) = self.mini_job.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Some(m)) => self.mini = Some((map, m)),
            Ok(None) => self.mini_failed = Some(map),
            Err(std::sync::mpsc::TryRecvError::Empty) => self.mini_job = Some((map, rx)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.mini_failed = Some(map),
        }
    }

    pub fn ymd(&self) -> (i32, i32, i32) {
        let mut d = self.day.clamp(1, days_in_year(self.year));
        for m in 1..=12 {
            let n = days_in_month(self.year, m);
            if d <= n {
                return (self.year, m, d);
            }
            d -= n;
        }
        (self.year, 12, 31)
    }

    pub fn set_ymd(&mut self, y: i32, m: i32, d: i32) {
        let y = y.clamp(1970, 2100);
        let m = m.clamp(1, 12);
        let d = d.clamp(1, days_in_month(y, m));
        self.year = y;
        self.day = (1..m).map(|k| days_in_month(y, k)).sum::<i32>() + d;
    }

    pub fn date_text(&self) -> String {
        let (y, m, d) = self.ymd();
        format!("{d:02}.{m:02}.{y}")
    }

    pub fn time_text(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    fn shift_day(&mut self, dir: i32) {
        let mut d = self.day + dir;
        if d < 1 {
            self.year = (self.year - 1).max(1970);
            d = days_in_year(self.year);
        } else if d > days_in_year(self.year) {
            self.year = (self.year + 1).min(2100);
            d = 1;
        }
        self.day = d;
    }

    pub fn set_now(&mut self) {
        let (y, m, d, h, min) = now_local();
        self.set_ymd(y, m, d);
        self.hour = h;
        self.minute = min;
        self.cal_year = self.year;
        self.cal_month = m - 1;
    }

    pub fn season_text(&self) -> String {
        let key = match self.season_mode {
            1 => "start.season.spring",
            2 => "start.season.summer",
            3 => "start.season.autumn",
            4 => "start.season.winter",
            _ => self.season_key(),
        };
        ::i18n::translate(key, &[])
    }

    pub fn season_arg(&self) -> Option<String> {
        ["spring", "summer", "autumn", "winter"]
            .get(self.season_mode.wrapping_sub(1))
            .map(|s| s.to_string())
    }

    pub fn cal_toggle(&mut self) {
        if !self.cal_open {
            let (y, m, _) = self.ymd();
            self.cal_year = y;
            self.cal_month = m - 1;
        }
        self.cal_open = !self.cal_open;
    }

    pub fn cal_shift(&mut self, dir: i32) {
        let mut m = self.cal_month + dir;
        let mut y = self.cal_year;
        if m < 0 {
            m = 11;
            y -= 1;
        } else if m > 11 {
            m = 0;
            y += 1;
        }
        self.cal_year = y.clamp(1970, 2100);
        self.cal_month = m;
    }

    pub fn cal_title(&self) -> String {
        format!(
            "{} {}",
            ::i18n::translate(&format!("start.cal.m{}", self.cal_month + 1), &[]),
            self.cal_year
        )
    }

    fn cal_date(&self, k: i32) -> (i32, i32, i32, bool) {
        let (y, m) = (self.cal_year, self.cal_month + 1);
        let first = weekday(y, m, 1);
        let off = k - first + 1;
        let dim = days_in_month(y, m);
        if off < 1 {
            let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
            (py, pm, days_in_month(py, pm) + off, false)
        } else if off > dim {
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            (ny, nm, off - dim, false)
        } else {
            (y, m, off, true)
        }
    }

    pub fn cal_cells(&self) -> Vec<(i32, bool, bool)> {
        let sel = self.ymd();
        (0..42)
            .map(|k| {
                let (y, m, d, inside) = self.cal_date(k);
                (d, inside, (y, m, d) == sel)
            })
            .collect()
    }

    pub fn cal_pick(&mut self, k: usize) {
        let (y, m, d, _) = self.cal_date(k as i32);
        self.set_ymd(y, m, d);
        self.cal_open = false;
    }

    fn apply_time(&mut self) {
        self.time_buf.retain(|c| c.is_ascii_digit());
        self.time_buf.truncate(4);
        let d: Vec<i32> = self.time_buf.chars().filter_map(|c| c.to_digit(10)).map(|x| x as i32).collect();
        let (h, m) = match d.len() {
            0 => return,
            1 => (d[0], 0),
            2 => (d[0] * 10 + d[1], 0),
            3 => (d[0], d[1] * 10 + d[2]),
            _ => (d[0] * 10 + d[1], d[2] * 10 + d[3]),
        };
        self.hour = h.min(23);
        self.minute = m.min(59);
    }

    pub fn step_minute(&mut self, dir: i32) {
        self.minute = (self.minute + dir).rem_euclid(60);
    }

    pub fn weather_label(&self, i: usize) -> String {
        let key = match i {
            0 => "start.weather.default",
            1 => "start.weather.cycle",
            2 => "start.weather.real",
            3 => "start.weather.custom",
            _ => return self.weathers.get(i).map(|w| w.0.clone()).unwrap_or_else(|| "-".to_string()),
        };
        ::i18n::translate(key, &[])
    }

    fn weather_icon(&self, i: usize) -> &'static str {
        match i {
            0 => "map",
            1 => "autorenew",
            2 => "public",
            3 => "tune",
            _ => {
                let n = self.weathers.get(i).map(|w| w.0.to_lowercase()).unwrap_or_default();
                if n.contains("snow") || n.contains("schnee") {
                    "ac_unit"
                } else if n.contains("rain") || n.contains("regen") || n.contains("storm") || n.contains("gewitter") {
                    "water_drop"
                } else if n.contains("nebel") || n.contains("fog") || n.contains("cloud") || n.contains("wolk")
                    || n.contains("overcast") || n.contains("bedeckt")
                {
                    "cloud"
                } else {
                    "wb_sunny"
                }
            }
        }
    }

    pub fn weather_rows(&self) -> Vec<(String, String)> {
        (0..self.weathers.len())
            .map(|i| (self.weather_icon(i).to_string(), self.weather_label(i)))
            .collect()
    }

    pub fn weather_step(&mut self, dir: i32) {
        let n = self.weathers.len() as i32;
        if n > 0 {
            self.weather = (self.weather as i32 + dir).clamp(0, n - 1) as usize;
        }
    }

    pub fn weather_arg(&self) -> Option<String> {
        match self.weather {
            0 => None,
            2 => Some(format!(
                "{}:{}",
                if self.metar_live { "metar-sync" } else { "metar-once" },
                self.icao_valid().unwrap_or_default()
            )),
            3 => Some(self.custom.encode()),
            w => self.weathers.get(w).map(|x| x.1.clone()),
        }
    }

    pub fn weather_file(&self) -> String {
        match self.weather {
            w if w >= WEATHER_MODES => self.weathers.get(w).map(|x| x.1.clone()).unwrap_or_default(),
            _ => String::new(),
        }
    }

    pub fn custom_rows(&self) -> Vec<(String, String)> {
        let tr = |k: &str| ::i18n::translate(k, &[]);
        let c = &self.custom;
        let vis = if c.visibility_m >= 1000.0 {
            format!("{:.0} km", c.visibility_m / 1000.0)
        } else {
            format!("{:.0} m", c.visibility_m)
        };
        vec![
            (tr("start.weather.vis"), vis),
            (tr("start.weather.clouds"), crate::weather_setup::CUSTOM_CLOUDS[c.cloud].to_string()),
            (
                tr("start.weather.precip"),
                crate::weather_setup::CUSTOM_PRECIP[c.precip.clamp(0, 2) as usize].to_string(),
            ),
            (tr("start.weather.temp"), format!("{:.0} \u{b0}C", c.temp_c)),
        ]
    }

    const CUSTOM_VIS: [f32; 8] = [200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0, 50000.0];

    pub fn custom_fracs(&self) -> Vec<f32> {
        let c = &self.custom;
        let vis = Self::CUSTOM_VIS.iter().position(|v| *v >= c.visibility_m).unwrap_or(7);
        vec![
            vis as f32 / 7.0,
            c.cloud as f32 / (crate::weather_setup::CUSTOM_CLOUDS.len() - 1).max(1) as f32,
            c.precip.clamp(0, 2) as f32 / 2.0,
            ((c.temp_c + 30.0) / 75.0).clamp(0.0, 1.0),
        ]
    }

    pub fn custom_set(&mut self, i: usize, f: f32) {
        let f = f.clamp(0.0, 1.0);
        let c = &mut self.custom;
        match i {
            0 => c.visibility_m = Self::CUSTOM_VIS[(f * 7.0).round() as usize],
            1 => c.cloud = (f * (crate::weather_setup::CUSTOM_CLOUDS.len() - 1) as f32).round() as usize,
            2 => {
                c.precip = (f * 2.0).round() as i32;
                c.road_wetness = if c.precip == 1 { 0.8 } else { 0.0 };
                c.snow_cover = c.precip == 2;
                c.snow_on_road = c.precip == 2;
            }
            _ => c.temp_c = (f * 75.0 - 30.0).round(),
        }
        c.normalize();
    }

    fn custom_step(&mut self, i: usize, dir: i32) {
        const VIS: [f32; 8] = [200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0, 50000.0];
        let c = &mut self.custom;
        match i {
            0 => {
                let at = VIS.iter().position(|v| *v >= c.visibility_m).unwrap_or(7) as i32;
                c.visibility_m = VIS[(at + dir).clamp(0, 7) as usize];
            }
            1 => c.cloud = (c.cloud as i32 + dir).rem_euclid(crate::weather_setup::CUSTOM_CLOUDS.len() as i32) as usize,
            2 => {
                c.precip = (c.precip + dir).rem_euclid(3);
                c.road_wetness = if c.precip == 1 { 0.8 } else { 0.0 };
                c.snow_cover = c.precip == 2;
                c.snow_on_road = c.precip == 2;
            }
            _ => c.temp_c = (c.temp_c + dir as f32).clamp(-30.0, 45.0),
        }
        c.normalize();
    }

    pub fn season_key(&self) -> &'static str {
        match self.day {
            1..=59 => "start.season.winter",
            60..=120 => "start.season.spring",
            121..=243 => "start.season.summer",
            244..=304 => "start.season.autumn",
            _ => "start.season.winter",
        }
    }

    pub fn step(&mut self, row: usize, dir: i32) {
        let step = |v: usize, n: usize| -> usize {
            if n == 0 {
                0
            } else {
                ((v as i32 + dir).rem_euclid(n as i32)) as usize
            }
        };
        match row {
            0 => self.vehicle_step(dir),
            8 | 11 => self.livery = step(self.livery, self.liveries.len() + 1),
            9 => self.depot = step(self.depot, self.depots.len() + 1),
            10 => self.number = step(self.number, self.numbers.len() + 1),
            1 => self.hour = (self.hour + dir).rem_euclid(24),
            12..=15 => self.custom_step(row - 12, dir),
            2 => self.traffic = (self.traffic as i32 + dir * 10).clamp(0, 200) as usize,
            3 => self.passengers = !self.passengers,
            4 => self.schedule = !self.schedule,
            5 => self.weather = step(self.weather, self.weathers.len()),
            6 => self.shift_day(dir),
            7 => self.situation = step(self.situation, self.situations.len()),
            _ => {}
        }
    }

    pub fn sect_rows(sect: usize) -> &'static [usize] {
        match sect {
            1 => &[7],
            3 => &[12, 13, 14, 15],
            _ => &[],
        }
    }

    pub fn open_sect(&mut self, sect: usize) {
        self.sect = sect.min(3);
        if self.sect > 0 {
            self.focus = self.sect - 1;
        }
        self.zone = if matches!(self.sect, 1 | 2 | 3) { 0 } else { 1 };
        self.cal_open = false;
        self.drop = 0;
        if let Some(r) = Self::sect_rows(self.sect).first() {
            self.row = *r;
        }
    }

    pub fn vehicle_names(&self) -> Vec<String> {
        self.vehicles.iter().map(|v| v.0.clone()).collect()
    }

    pub fn vehicle_step(&mut self, dir: i32) {
        let order: Vec<usize> = self.veh_groups().into_iter().flat_map(|g| g.2).collect();
        let n = order.len() as i32;
        if n == 0 {
            return;
        }
        let at = order.iter().position(|i| *i == self.vehicle);
        let next = match at {
            Some(p) => (p as i32 + dir).clamp(0, n - 1) as usize,
            None => 0,
        };
        self.vehicle = order[next];
        self.veh_open = self.maker_key(self.vehicle);
    }

    pub fn entry_rows(&self) -> Vec<(u32, String)> {
        self.map_entries.get(self.map).cloned().unwrap_or_default()
    }

    pub fn entry_chosen(&self) -> Option<u32> {
        let map = self.maps.get(self.map)?;
        self.entry.filter(|_| self.entry_for == map.1)
    }

    pub fn entry_shown(&self) -> u32 {
        self.entry_chosen()
            .or_else(|| self.map_entries.get(self.map).and_then(|e| e.first()).map(|r| r.0))
            .unwrap_or(0)
    }

    pub fn entry_text(&self) -> String {
        let shown = self.entry_shown();
        self.entry_rows().into_iter().find(|r| r.0 == shown).map_or_else(|| "-".to_string(), |r| r.1)
    }

    pub fn entry_pick(&mut self, index: u32) {
        if let Some(map) = self.maps.get(self.map) {
            self.entry_for = map.1.clone();
            self.entry = Some(index);
        }
    }

    pub fn entry_step(&mut self, dir: i32) {
        let rows = self.entry_rows();
        if rows.is_empty() {
            return;
        }
        let cur = self.entry_shown();
        let at = rows.iter().position(|r| r.0 == cur).unwrap_or(0) as i32;
        let next = (at + dir).clamp(0, rows.len() as i32 - 1) as usize;
        self.entry_pick(rows[next].0);
    }

    pub fn map_step(&mut self, dir: i32) {
        let n = self.maps.len() as i32;
        if n > 0 {
            self.map = (self.map as i32 + dir).clamp(0, n - 1) as usize;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{auto_row, icao_code};

    #[test]
    fn a_remembered_choice_comes_back_on_its_own_row() {
        let liveries = ["GPM STIF v1", "GPM STIF v2", "Keolis"].map(String::from);
        // (what `remember` writes for the row picked: the item before it, "Auto" being row 0)
        for row in 1..=liveries.len() {
            let saved = &liveries[row - 1];
            assert_eq!(auto_row(&liveries, |l| l == saved), row);
        }
        assert_eq!(auto_row(&liveries, |l| l.is_empty()), 0);
    }

    #[test]
    fn accepts_four_character_icao_codes_with_digits() {
        assert_eq!(icao_code("koj4"), Some("KOJ4".into()));
        assert_eq!(icao_code("KJFK"), Some("KJFK".into()));
        assert_eq!(icao_code("KOJ"), None);
        assert_eq!(icao_code("KOJ4!"), None);
    }
}
