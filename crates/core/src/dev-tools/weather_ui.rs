#![allow(unused_imports)]
use super::types::*;
use super::util::*;
use crate::weather_setup::{CUSTOM_CLOUDS, CUSTOM_PRECIP, CustomWeather};
use ::content::weather::Weather;
use imgui::Condition;

const SCENARIOS: [&str; 10] = [
    "Clear", "Fair", "Cloudy", "Overcast", "Drizzle", "Rain", "Storm", "Snow", "Fog", "Heat",
];

pub(super) struct WeatherTool {
    presets: Vec<(String, Weather)>,
    loaded: bool,
    filter: String,
    selected: Option<usize>,
    blend_secs: f32,
    edit: CustomWeather,
    edit_init: bool,
    live: bool,
    icao: String,
    icao_init: bool,
    save_name: String,
    rng: u64,
}

impl WeatherTool {
    pub(super) fn new() -> WeatherTool {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9);
        WeatherTool {
            presets: Vec::new(),
            loaded: false,
            filter: String::new(),
            selected: None,
            blend_secs: 30.0,
            edit: CustomWeather::default(),
            edit_init: false,
            live: true,
            icao: String::new(),
            icao_init: false,
            save_name: "My Weather".into(),
            rng: seed | 1,
        }
    }

    fn reload(&mut self) {
        self.presets = crate::weather_cycle::installed();
        self.loaded = true;
        self.selected = None;
    }

    fn rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
}

pub(super) fn window(
    ui: &imgui::Ui,
    open: &mut bool,
    tool: &mut WeatherTool,
    extra: &Extra,
    actions: &mut Vec<Action>,
) {
    if !*open {
        return;
    }
    if !tool.loaded {
        tool.reload();
    }
    let wi = &extra.weather;
    if !tool.edit_init {
        tool.edit = wi.custom.clone();
        tool.edit_init = true;
    }
    if !tool.icao_init {
        tool.icao = wi.metar_station.clone();
        tool.icao_init = true;
    }
    ui.window("Weather")
        .opened(open)
        .size([500.0, 680.0], Condition::FirstUseEver)
        .position([12.0, 32.0], Condition::FirstUseEver)
        .build(|| {
            status(ui, wi);
            ui.separator();
            if let Some(_bar) = ui.tab_bar("##weather_tabs") {
                if let Some(_t) = ui.tab_item("Presets") {
                    presets(ui, tool, wi, actions);
                }
                if let Some(_t) = ui.tab_item("Custom") {
                    custom(ui, tool, wi, actions);
                }
                if let Some(_t) = ui.tab_item("Live") {
                    live(ui, tool, wi, extra, actions);
                }
            }
        });
}

fn beaufort(ms: f32) -> i32 {
    ((ms.max(0.0) / 0.836).powf(2.0 / 3.0)).round().clamp(0.0, 12.0) as i32
}

fn compass(deg: f32) -> &'static str {
    const N: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    N[(((deg.rem_euclid(360.0) + 22.5) / 45.0) as usize) % 8]
}

fn cloud_name(w: &Weather) -> String {
    let t = w.clouds.0.trim();
    if t.is_empty() || t.starts_with("-1") {
        "none".to_string()
    } else {
        format!("{t}, base {:.0} m", w.clouds.1)
    }
}

fn precip_name(kind: i32) -> &'static str {
    match kind {
        1 => "rain",
        2 => "snow",
        _ => "none",
    }
}

fn source(wi: &WeatherInfo) -> String {
    let s = wi.spec.trim();
    if s.is_empty() {
        "map default".to_string()
    } else if wi.is_custom {
        "custom".to_string()
    } else if let Some(c) = s.strip_prefix("metar:") {
        format!("METAR {c}")
    } else if s.starts_with(crate::weather_setup::REPORT) {
        "METAR report (host)".to_string()
    } else {
        s.to_string()
    }
}

fn status(ui: &imgui::Ui, wi: &WeatherInfo) {
    let w = &wi.weather;
    let name = if w.name.is_empty() { "(unnamed)" } else { &w.name };
    ui.text(format!("{name}"));
    ui.same_line();
    ui.text_disabled(format!("[{}]", source(wi)));
    if wi.client {
        ui.text_colored([1.0, 0.6, 0.2, 1.0], "LAN client: the host sets the weather");
    } else if wi.metar_locked {
        ui.text_colored(
            [1.0, 0.6, 0.2, 1.0],
            "METAR sync is on: weather changes are blocked (Live tab)",
        );
    }
    if let Some((k, to)) = wi.blend.as_ref() {
        imgui::ProgressBar::new(*k)
            .size([-1.0, 0.0])
            .overlay_text(format!("-> {to}  {:.0}%", k * 100.0))
            .build(ui);
    }
    let rh = crate::weather_setup::relative_humidity(w.temp.0, w.temp.1);
    let dew = crate::weather_setup::dew_point_c(w.temp.0, rh);
    ui.text(format!(
        "Visibility {:.0} m   Temp {:.1} C   Humidity {:.0}%   Dew {:.1} C",
        w.fog.0, w.temp.0, rh, dew
    ));
    ui.text(format!(
        "Wind {:.0} deg ({}) {:.1} m/s = {} Bft   Pressure {:.0} hPa",
        w.wind.0,
        compass(w.wind.0),
        w.wind.1,
        beaufort(w.wind.1),
        w.pressure
    ));
    ui.text(format!(
        "Clouds: {}   Precip: {} {:.0}%",
        cloud_name(w),
        precip_name(wi.precip.0),
        wi.precip.1 * 100.0
    ));
    ui.text(format!(
        "Cloud density {:.2}   Drift {:.0} / {:.0}",
        wi.density, wi.drift[0], wi.drift[1]
    ));
    for (i, l) in wi.layers.iter().enumerate() {
        ui.text_disabled(format!(
            "Layer {}: {:.2} {:.2} {:.2} {:.2}",
            i + 1,
            l[0],
            l[1],
            l[2],
            l[3]
        ));
    }
    ui.text(format!(
        "Road wetness {:.0}%   Street cond {:.2}   Snow {}{}",
        wi.wetness * 100.0,
        wi.street_cond,
        if w.snow { "yes" } else { "no" },
        if w.snow_on_road { ", on road" } else { "" }
    ));
}

fn summary(w: &Weather) -> String {
    let vis = if w.fog.0 >= 10_000.0 {
        format!("{:.0} km", w.fog.0 / 1000.0)
    } else {
        format!("{:.0} m", w.fog.0)
    };
    let kind = w.precip.first().copied().unwrap_or(0.0) as i32;
    let clouds = {
        let t = w.clouds.0.trim();
        if t.starts_with("-1") || t.is_empty() { "clear" } else { t }
    };
    format!(
        "{vis}, {:.0} C, {clouds}{}",
        w.temp.0,
        match kind {
            1 => ", rain",
            2 => ", snow",
            _ => "",
        }
    )
}

fn same_file(a: &str, b: &str) -> bool {
    a.replace('\\', "/").eq_ignore_ascii_case(&b.replace('\\', "/"))
}

fn presets(ui: &imgui::Ui, tool: &mut WeatherTool, wi: &WeatherInfo, actions: &mut Vec<Action>) {
    let locked = wi.client || wi.metar_locked;
    if ui.button("Reload list") {
        tool.reload();
    }
    ui.same_line();
    ui.text_disabled(format!("{} installed (Weather/*.owt)", tool.presets.len()));
    ui.set_next_item_width(-1.0);
    ui.input_text("##wfilter", &mut tool.filter)
        .hint("Filter by name or file")
        .build();
    ui.set_next_item_width(-120.0);
    ui.slider_config("Blend (s of day)##wb", 0.5, 600.0)
        .flags(imgui::SliderFlags::LOGARITHMIC)
        .display_format("%.1f s")
        .build(&mut tool.blend_secs);
    let filter = tool.filter.trim().to_ascii_lowercase();
    let mut apply: Option<(String, f32)> = None;
    let mut select: Option<usize> = None;
    ui.child_window("##wlist")
        .size([0.0, 250.0])
        .border(true)
        .build(|| {
            for (i, (file, w)) in tool.presets.iter().enumerate() {
                let label = if w.name.is_empty() { file.as_str() } else { w.name.as_str() };
                if !filter.is_empty()
                    && !label.to_ascii_lowercase().contains(&filter)
                    && !file.to_ascii_lowercase().contains(&filter)
                {
                    continue;
                }
                let current = same_file(file, &wi.spec);
                let text = format!(
                    "{}{}  -  {}##p{i}",
                    if current { "* " } else { "" },
                    label,
                    summary(w)
                );
                if ui
                    .selectable_config(&text)
                    .selected(tool.selected == Some(i))
                    .build()
                {
                    select = Some(i);
                }
                if ui.is_item_hovered() {
                    if ui.is_mouse_double_clicked(imgui::MouseButton::Left) {
                        apply = Some((file.clone(), tool.blend_secs));
                    }
                    ui.tooltip(|| {
                        ui.text(file);
                        if !w.description.trim().is_empty() {
                            ui.separator();
                            ui.text_wrapped(w.description.trim());
                        }
                    });
                }
            }
        });
    if select.is_some() {
        tool.selected = select;
    }
    let sel = tool.selected.and_then(|i| tool.presets.get(i)).cloned();
    match sel.as_ref() {
        Some((file, w)) => {
            ui.text(format!("Selected: {file}"));
            ui.text_disabled(summary(w));
            ui.text_disabled(format!(
                "wind {:.0} deg {:.1} m/s, {:.0} hPa, clouds {}, wet {:.0}%",
                w.wind.0,
                w.wind.1,
                w.pressure,
                cloud_name(w),
                w.ground_wet[0] / 255.0 * 100.0
            ));
            if !w.description.trim().is_empty() {
                ui.text_wrapped(w.description.trim());
            }
        }
        None => ui.text_disabled("Select a weather (double-click applies it)"),
    }
    ui.disabled(locked || sel.is_none(), || {
        if ui.button("Apply (blend)") {
            if let Some((f, _)) = sel.as_ref() {
                apply = Some((f.clone(), tool.blend_secs));
            }
        }
        ui.same_line();
        if ui.button("Apply now") {
            if let Some((f, _)) = sel.as_ref() {
                apply = Some((f.clone(), 0.5));
            }
        }
    });
    ui.same_line();
    ui.disabled(locked, || {
        if ui.button("Next preset") {
            actions.push(Action::WeatherNext);
        }
    });
    if ui.button("Load selected into Custom editor") {
        if let Some((_, w)) = sel.as_ref() {
            tool.edit = CustomWeather::from_weather(w, tool.edit.brightness, wi.wetness);
            tool.edit_init = true;
        }
    }
    if let Some((f, s)) = apply {
        actions.push(Action::WeatherPreset(f, s));
    }
}

fn scenario(i: usize, base: &CustomWeather) -> CustomWeather {
    let mut c = CustomWeather {
        wind_dir: base.wind_dir,
        brightness: 1.0,
        ..CustomWeather::default()
    };
    match i {
        0 => {}
        1 => {
            c.cloud = 1;
            c.cloud_base_m = 1500.0;
            c.visibility_m = 30_000.0;
            c.wind_speed = 3.0;
            c.temp_c = 20.0;
        }
        2 => {
            c.cloud = 3;
            c.cloud_base_m = 900.0;
            c.visibility_m = 15_000.0;
            c.wind_speed = 5.0;
            c.humidity = 70.0;
            c.pressure = 1005.0;
            c.brightness = 0.85;
        }
        3 => {
            c.cloud = 4;
            c.cloud_base_m = 500.0;
            c.visibility_m = 8_000.0;
            c.wind_speed = 4.0;
            c.humidity = 85.0;
            c.pressure = 1000.0;
            c.temp_c = 10.0;
            c.brightness = 0.7;
        }
        4 => {
            c.cloud = 4;
            c.cloud_base_m = 350.0;
            c.visibility_m = 4_000.0;
            c.wind_speed = 3.0;
            c.humidity = 92.0;
            c.pressure = 1004.0;
            c.temp_c = 11.0;
            c.precip = 1;
            c.precip_intensity = 40.0;
            c.road_wetness = 0.6;
            c.brightness = 0.7;
        }
        5 => {
            c.cloud = 4;
            c.cloud_base_m = 300.0;
            c.visibility_m = 2_500.0;
            c.wind_speed = 7.0;
            c.humidity = 95.0;
            c.pressure = 998.0;
            c.temp_c = 12.0;
            c.precip = 1;
            c.precip_intensity = 130.0;
            c.road_wetness = 1.0;
            c.brightness = 0.6;
        }
        6 => {
            c.cloud = 4;
            c.cloud_base_m = 250.0;
            c.visibility_m = 1_200.0;
            c.wind_speed = 18.0;
            c.humidity = 98.0;
            c.pressure = 982.0;
            c.temp_c = 14.0;
            c.precip = 1;
            c.precip_intensity = 255.0;
            c.road_wetness = 1.0;
            c.brightness = 0.4;
        }
        7 => {
            c.cloud = 4;
            c.cloud_base_m = 350.0;
            c.visibility_m = 1_800.0;
            c.wind_speed = 4.0;
            c.humidity = 90.0;
            c.pressure = 1005.0;
            c.temp_c = -3.0;
            c.precip = 2;
            c.precip_intensity = 120.0;
            c.snow_cover = true;
            c.snow_on_road = true;
            c.road_wetness = 0.3;
            c.brightness = 0.8;
        }
        8 => {
            c.cloud = 4;
            c.cloud_base_m = 100.0;
            c.visibility_m = 120.0;
            c.wind_speed = 0.5;
            c.humidity = 100.0;
            c.pressure = 1020.0;
            c.temp_c = 3.0;
            c.road_wetness = 0.4;
            c.brightness = 0.8;
        }
        _ => {
            c.cloud = 0;
            c.visibility_m = 40_000.0;
            c.wind_speed = 1.0;
            c.humidity = 30.0;
            c.pressure = 1018.0;
            c.temp_c = 36.0;
        }
    }
    c
}

fn custom(ui: &imgui::Ui, tool: &mut WeatherTool, wi: &WeatherInfo, actions: &mut Vec<Action>) {
    let locked = wi.client || wi.metar_locked;
    ui.text("Scenarios");
    for (i, n) in SCENARIOS.iter().enumerate() {
        if i % 5 != 0 {
            ui.same_line();
        }
        if ui.button(format!("{n}##sc{i}")) {
            tool.edit = scenario(i, &tool.edit);
            if !locked {
                actions.push(Action::WeatherCustom(Box::new(tool.edit.clone())));
            }
        }
    }
    if ui.button("From current") && !locked {
        actions.push(Action::WeatherFromCurrent);
    }
    ui.same_line();
    if ui.button("Randomize") {
        let mut r = |a: f32, b: f32| a + (b - a) * tool.rand();
        let rain = r(0.0, 1.0);
        let cold = r(0.0, 1.0) < 0.2;
        let mut c = CustomWeather {
            visibility_m: (50.0f32.ln() + (50_000.0f32.ln() - 50.0f32.ln()) * r(0.15, 1.0)).exp(),
            brightness: r(0.5, 1.0),
            wind_dir: r(0.0, 360.0),
            wind_speed: r(0.0, 14.0),
            temp_c: if cold { r(-8.0, 2.0) } else { r(2.0, 30.0) },
            humidity: r(30.0, 100.0),
            pressure: r(985.0, 1030.0),
            cloud: (r(0.0, 5.0) as usize).min(4),
            cloud_base_m: r(250.0, 3000.0),
            ..CustomWeather::default()
        };
        if rain > 0.55 {
            c.cloud = 4;
            c.precip = if c.temp_c < 1.5 { 2 } else { 1 };
            c.precip_intensity = r(20.0, 255.0);
            c.road_wetness = if c.precip == 1 { r(0.4, 1.0) } else { r(0.0, 0.4) };
            c.snow_cover = c.precip == 2;
            c.snow_on_road = c.precip == 2 && r(0.0, 1.0) > 0.5;
            c.visibility_m = c.visibility_m.min(8_000.0);
        }
        c.normalize();
        tool.edit = c;
        if !locked {
            actions.push(Action::WeatherCustom(Box::new(tool.edit.clone())));
        }
    }
    ui.same_line();
    if ui.button("Load current") {
        tool.edit = wi.custom.clone();
    }
    ui.same_line();
    if ui.button("Defaults") {
        tool.edit = CustomWeather::default();
    }
    ui.checkbox("Live apply", &mut tool.live);
    ui.same_line();
    ui.disabled(locked, || {
        if ui.button("Apply") {
            actions.push(Action::WeatherCustom(Box::new(tool.edit.clone())));
        }
    });
    ui.separator();

    let before = tool.edit.clone();
    let e = &mut tool.edit;
    ui.set_next_item_width(-150.0);
    ui.slider_config("Visibility##cw", 50.0, 50_000.0)
        .flags(imgui::SliderFlags::LOGARITHMIC)
        .display_format("%.0f m")
        .build(&mut e.visibility_m);
    ui.set_next_item_width(-150.0);
    ui.slider_config("Brightness##cw", 0.0, 1.5)
        .display_format("%.2f")
        .build(&mut e.brightness);
    ui.set_next_item_width(-150.0);
    ui.slider_config("Wind direction##cw", 0.0, 360.0)
        .display_format("%.0f deg")
        .build(&mut e.wind_dir);
    ui.same_line();
    ui.text(compass(e.wind_dir));
    ui.set_next_item_width(-150.0);
    ui.slider_config("Wind speed##cw", 0.0, 50.0)
        .display_format("%.1f m/s")
        .build(&mut e.wind_speed);
    ui.same_line();
    ui.text(format!("{} Bft", beaufort(e.wind_speed)));
    ui.set_next_item_width(-150.0);
    ui.slider_config("Temperature##cw", -40.0, 50.0)
        .display_format("%.1f C")
        .build(&mut e.temp_c);
    ui.set_next_item_width(-150.0);
    ui.slider_config("Humidity##cw", 0.0, 100.0)
        .display_format("%.0f %%")
        .build(&mut e.humidity);
    ui.same_line();
    ui.text(format!(
        "dew {:.1}",
        crate::weather_setup::dew_point_c(e.temp_c, e.humidity)
    ));
    ui.set_next_item_width(-150.0);
    ui.slider_config("Pressure##cw", 900.0, 1100.0)
        .display_format("%.0f hPa")
        .build(&mut e.pressure);
    ui.set_next_item_width(-150.0);
    ui.combo_simple_string("Cloud type##cw", &mut e.cloud, &CUSTOM_CLOUDS[..]);
    ui.set_next_item_width(-150.0);
    ui.slider_config("Cloud base##cw", 50.0, 5000.0)
        .display_format("%.0f m")
        .build(&mut e.cloud_base_m);
    let mut p = e.precip.clamp(0, 2) as usize;
    ui.set_next_item_width(-150.0);
    if ui.combo_simple_string("Precipitation##cw", &mut p, &CUSTOM_PRECIP[..]) {
        e.precip = p as i32;
        if p == 2 {
            e.snow_cover = true;
        }
    }
    ui.set_next_item_width(-150.0);
    ui.slider_config("Intensity##cw", 0.0, 255.0)
        .display_format("%.0f")
        .build(&mut e.precip_intensity);
    ui.set_next_item_width(-150.0);
    ui.slider_config("Road wetness##cw", 0.0, 1.0)
        .display_format("%.2f")
        .build(&mut e.road_wetness);
    ui.checkbox("Snow cover##cw", &mut e.snow_cover);
    ui.same_line();
    ui.checkbox("Snow on road##cw", &mut e.snow_on_road);
    let changed = *e != before;
    if changed && tool.live && !locked {
        actions.push(Action::WeatherCustom(Box::new(tool.edit.clone())));
    }
    ui.separator();
    let mut spec = tool.edit.encode();
    ui.set_next_item_width(-1.0);
    ui.input_text("##wspec", &mut spec).read_only(true).build();
    ui.text_disabled("Custom spec (usable as weather = ...)");
    ui.separator();
    ui.set_next_item_width(-190.0);
    ui.input_text("##wsave", &mut tool.save_name).build();
    ui.same_line();
    if ui.button("Save current as .owt") {
        actions.push(Action::WeatherSave(tool.save_name.clone()));
    }
}

fn mmss(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn live(
    ui: &imgui::Ui,
    tool: &mut WeatherTool,
    wi: &WeatherInfo,
    extra: &Extra,
    actions: &mut Vec<Action>,
) {
    let locked = wi.client || wi.metar_locked;

    ui.text("Road");
    let mut wet = wi.wetness;
    ui.set_next_item_width(-150.0);
    if ui
        .slider_config("Road wetness##lw", 0.0, 1.0)
        .display_format("%.2f")
        .build(&mut wet)
    {
        actions.push(Action::WeatherWetness(wet));
    }
    if ui.button("Dry") {
        actions.push(Action::WeatherWetness(0.0));
    }
    ui.same_line();
    if ui.button("Soaked") {
        actions.push(Action::WeatherWetness(1.0));
    }
    ui.text_disabled("Rain soaks the roads in minutes, sunshine dries them in ~20 min.");
    ui.separator();

    ui.text("Weather cycle");
    let mut cycle = wi.cycle_next.is_some();
    ui.disabled(wi.client || wi.metar_locked, || {
        if ui.checkbox("Cycle through installed weathers", &mut cycle) {
            actions.push(Action::WeatherCycle(cycle));
        }
    });
    if let Some(n) = wi.cycle_next {
        ui.text(format!("Next change in {} (of the day)", mmss(n)));
        ui.same_line();
        if ui.button("Change now") {
            actions.push(Action::WeatherCycleNow);
        }
    }
    ui.separator();

    ui.text("METAR (real weather)");
    ui.set_next_item_width(100.0);
    ui.input_text("ICAO##lw", &mut tool.icao).build();
    ui.same_line();
    ui.disabled(locked || wi.metar_loading, || {
        if ui.button("Load once") {
            actions.push(Action::WeatherMetar(tool.icao.clone()));
        }
    });
    if wi.metar_loading {
        ui.same_line();
        ui.text_disabled("loading...");
    }
    let mut sync = ::config::get_bool("gameplay", "metar_sync").unwrap_or(false);
    if ui.checkbox("METAR sync (game setting)", &mut sync) {
        ::config::set_setting("gameplay", "metar_sync", sync);
    }
    ui.separator();

    ui.text("Time and date");
    if wi.time_locked {
        ui.text_colored(
            [1.0, 0.6, 0.2, 1.0],
            "Time is locked (real time sync or LAN client)",
        );
    }
    ui.disabled(wi.time_locked, || {
        let mut h = (extra.clock / 3600.0) as f32;
        ui.set_next_item_width(-150.0);
        if ui
            .slider_config("Time of day##lw", 0.0, 24.0)
            .display_format("%.2f h")
            .build(&mut h)
        {
            actions.push(Action::SetTime(h as f64 * 3600.0));
        }
        for (i, (n, t)) in [("Midnight", 0.0), ("Dawn", 6.0), ("Noon", 12.0), ("Dusk", 19.0)]
            .iter()
            .enumerate()
        {
            if i > 0 {
                ui.same_line();
            }
            if ui.button(format!("{n}##tm")) {
                actions.push(Action::SetTime(*t * 3600.0));
            }
        }
        let mut d = wi.day_of_year;
        ui.set_next_item_width(-150.0);
        if ui.slider("Day of year##lw", 1, 365, &mut d) {
            actions.push(Action::SetDay(d));
        }
        for (i, (n, d)) in [("Winter", 15), ("Spring", 105), ("Summer", 196), ("Autumn", 288)]
            .iter()
            .enumerate()
        {
            if i > 0 {
                ui.same_line();
            }
            if ui.button(format!("{n}##sd")) {
                actions.push(Action::SetDay(*d));
            }
        }
    });
    ui.text_disabled(format!(
        "{} {:02}.{:02}.{}  day {}{}",
        hms(extra.clock),
        wi.day_month.0,
        wi.day_month.1,
        wi.year,
        wi.day_of_year,
        if extra.paused { "  (paused)" } else { "" }
    ));
}

pub(super) fn to_owt(name: &str, w: &Weather) -> String {
    let p = |i: usize| w.precip.get(i).copied().unwrap_or(0.0);
    let mut s = String::new();
    s.push_str(&format!("[name]\n{name}\n"));
    s.push_str("[description]\nSaved from the weather tool\n[end]\n");
    s.push_str(&format!("[fog]\n{}\n{}\n", w.fog.0, w.fog.1));
    s.push_str(&format!("[wind]\n{}\n{}\n", w.wind.0, w.wind.1));
    s.push_str(&format!("[temp]\n{}\n{}\n", w.temp.0, w.temp.1));
    s.push_str(&format!("[press]\n{}\n", w.pressure));
    let clouds = if w.clouds.0.trim().is_empty() { "-1" } else { w.clouds.0.trim() };
    s.push_str(&format!("[clouds]\n{}\n{}\n", clouds, w.clouds.1));
    s.push_str(&format!(
        "[precip]\n{}\n{}\n{}\n{}\n{}\n",
        p(0),
        p(1),
        p(2),
        p(3),
        p(4)
    ));
    s.push_str(&format!(
        "[groundwet]\n{}\n{}\n{}\n",
        w.ground_wet[0], w.ground_wet[1], w.ground_wet[2]
    ));
    if w.snow {
        s.push_str("[snow]\n");
    }
    if w.snow_on_road {
        s.push_str("[snowonroad]\n");
    }
    s
}