//! World pages.

use super::*;

pub(crate) fn world_groups(
    app: &App,
) -> Vec<(
    String,
    Vec<(String, String)>,
    Vec<(String, Vec<(String, String)>)>,
)> {
    world_pages(app)
        .into_iter()
        .filter(|p| !p.1.is_empty())
        .map(|(title, rows)| {
            let (presets, rows): (Vec<_>, Vec<_>) = rows
                .into_iter()
                .partition(|r| r.1.starts_with("clock_set "));
            let subs = if presets.is_empty() {
                Vec::new()
            } else {
                vec![(tx("pause.world.text.presets"), presets)]
            };
            (title, rows, subs)
        })
        .collect()
}

pub(crate) fn world_pages(app: &App) -> Vec<Page> {
    let client = app
        .lan
        .as_ref()
        .is_some_and(|l| l.role == ::network::Role::Client);
    let pct = |v: f32| format!("{:.0} %", v * 100.0);
    let mut time: Vec<(String, String)> = Vec::new();
    let mut weather: Vec<(String, String)> = Vec::new();
    let mut climate: Vec<(String, String)> = Vec::new();
    let mut tools: Vec<(String, String)> = Vec::new();
    if !client {
        let t = app.clock.time;
        let now = format!(
            "{:02}:{:02}",
            ((t / 3600.0) as i64).rem_euclid(24),
            ((t / 60.0) as i64) % 60
        );
        time.extend(switch_row(
            Some(app),
            "time_sync",
            &tx("pause.world.text.real_time_sync"),
            &tx("pause.world.text.the_game_follows_your_device_s_date_and_time"),
        ));
        if app.real_time_locked() {
            let (d, m) = app.clock.day_month();
            let text = format!(
                "{:04}-{m:02}-{d:02}  {}:{:02}",
                app.clock.year,
                now,
                (t as i64) % 60
            );
            time.push((
                row(
                    &tx("pause.world.text.date_and_time"),
                    'i',
                    &text,
                    &tx("pause.world.text.synchronized_with_the_real_time"),
                    None,
                ),
                "noop".to_string(),
            ));
        } else {
            match app.menu_edit.as_ref() {
                Some(d) => {
                    let mut c: Vec<char> = d.chars().collect();
                    c.resize(6, '_');
                    let typed = format!("{}{}:{}{}:{}{}", c[0], c[1], c[2], c[3], c[4], c[5]);
                    time.push((
                        row(
                            &tx("pause.world.text.exact_time"),
                            'E',
                            &typed,
                            &tx("pause.world.text.press_enter_to_change_esc_to_cancel"),
                            None,
                        ),
                        "time_edit".to_string(),
                    ));
                }
                None => {
                    let secs = format!("{}:{:02}", now, (t as i64) % 60);
                    time.push((
                        row(
                            &tx("pause.world.text.exact_time"),
                            'e',
                            &secs,
                            &tx("pause.world.text.change_the_current_time_press_enter_to_change"),
                            None,
                        ),
                        "time_edit".to_string(),
                    ));
                }
            }
            time.extend(slider_row(
                Some(app),
                "hour",
                &tx("pause.world.text.hour"),
                &tx("pause.world.text.set_the_hour_of_the_day_directly"),
                &|v| format!("{:02}", v as i64),
            ));
            time.extend(slider_row(
                Some(app),
                "minute",
                &tx("pause.world.text.minute"),
                &tx("pause.world.text.set_the_minute_directly"),
                &|v| format!("{:02}", v as i64),
            ));
            for (name, hm, secs) in [
                (&tx("pause.world.text.morning"), "06:00", 6 * 3600),
                (&tx("pause.world.text.noon"), "12:00", 12 * 3600),
                (&tx("pause.world.text.evening"), "18:00", 18 * 3600),
                (&tx("pause.world.text.night"), "23:00", 23 * 3600),
            ] {
                time.push(button(
                    name,
                    hm,
                    &tx("pause.world.text.jump_to_this_time_of_day"),
                    &format!("clock_set {secs}"),
                ));
            }
            if let (Some(_), Some(p)) = (app.duty.as_ref(), app.player.as_ref()) {
                let d = p.vehicle.host.tt_delay as f64;
                if d.abs() >= 1.0 {
                    let text = format!(
                        "{}{}:{:02}",
                        if d < 0.0 { "−" } else { "+" },
                        (d.abs() / 60.0) as i64,
                        d.abs() as i64 % 60
                    );
                    time.push(button(
                        &tx("pause.world.text.on_time_with_the_timetable"),
                        &text,
                        &tx("pause.world.text.move_the_clock_so_that_the_vehicle_is_on_time"),
                        "clock_ontime",
                    ));
                }
            }
            if app.lan.is_none() {
                time.extend(slider_row(
                    Some(app),
                    "speed",
                    &tx("pause.world.text.time_speed"),
                    &tx("pause.world.text.how_fast_the_world_s_clock_runs"),
                    &|v| format!("x{v}"),
                ));
            }
        }
        weather.extend(switch_row(
            Some(app),
            "metar_sync",
            &tx("pause.world.text.metar_sync"),
            &tx("pause.world.text.the_weather_follows_the_real_metar_report"),
        ));
        let src = if ::config::get_string("gameplay", "metar_station")
            .unwrap_or_default()
            .is_empty()
        {
            format!(
                "{} ({})",
                app.metar_station(),
                ::user_interface::tr("automatic")
            )
        } else {
            app.metar_station()
        };
        weather.push((
            row(
                &tx("pause.world.text.metar_source"),
                'o',
                &src,
                &tx("pause.world.text.the_airport_used_for_real_weather"),
                None,
            ),
            "metar_src".to_string(),
        ));
        let typed = if app.menu_edit_icao {
            let mut s = app.menu_edit.clone().unwrap_or_default();
            while s.len() < 4 {
                s.push('_');
            }
            format!("{s}  (typing)")
        } else {
            app.metar_station()
        };
        weather.push((
            row(
                "ICAO",
                if app.menu_edit_icao { 'E' } else { 'e' },
                &typed,
                &tx("pause.world.text.enter_any_4_letter_icao_station"),
                None,
            ),
            "metar_icao_edit".to_string(),
        ));
        if app.metar_locked() {
            weather.push(button(
                &tx("pause.world.text.metar_report"),
                &tx("pause.world.text.refresh_now"),
                &tx("pause.world.text.fetch_the_selected_station_again_without_waiting_for_the_next_automatic_update"),
                "metar_refresh",
            ));
        } else {
            weather.push(button(
                &tx("pause.world.text.metar_report"),
                &tx("pause.world.text.load_once"),
                &tx("pause.world.text.load_the_selected_station_once_without_enabling_continuous_metar_sync"),
                "metar_once",
            ));
        }
        weather.push((
            row(
                &tx("pause.world.text.preset"),
                'o',
                &weather_name(app),
                &tx("pause.world.text.a_ready_made_weather"),
                None,
            ),
            "weather".to_string(),
        ));
        if !app.metar_locked() {
            weather.push(button(
                &tx("pause.world.text.custom_weather"),
                &tx("pause.world.text.edit_current"),
                &tx("pause.world.text.freeze_the_weather_currently_in_force_and_edit_it_as_a_custom_weather"),
                "weather_custom",
            ));
        }
        let cloud = app
            .weather
            .as_ref()
            .and_then(|w| cloud_index(&w.clouds.0))
            .map(|i| CLOUD_TYPES[i].1.to_string())
            .or_else(|| app.weather.as_ref().map(|w| w.clouds.0.trim().to_string()))
            .unwrap_or_default();
        weather.push((
            row(
                &tx("pause.world.text.clouds"),
                'o',
                &cloud,
                &tx("pause.world.text.the_kind_of_clouds_in_the_sky"),
                None,
            ),
            "cloudkind".to_string(),
        ));
        weather.extend(slider_row(
            Some(app),
            "visibility",
            &tx("pause.world.text.visibility"),
            &tx("pause.world.text.how_far_one_can_see_less_is_fog"),
            &|v| {
                if v >= 1000.0 {
                    format!("{:.1} km", v / 1000.0)
                } else {
                    format!("{} m", v as i64)
                }
            },
        ));
        weather.extend(slider_row(
            Some(app),
            "brightness",
            &tx("pause.world.text.brightness"),
            &tx("pause.world.text.brightness_of_the_custom_weather_lighting"),
            &|v| format!("{:.0} %", v * 100.0),
        ));
        let kind = app
            .weather
            .as_ref()
            .map(|w| {
                (w.precip.first().copied().unwrap_or(0.0).max(0.0) as usize)
                    .min(PRECIP_KINDS.len() - 1)
            })
            .unwrap_or(0);
        weather.push((
            row(
                &tx("pause.world.text.precipitation"),
                'o',
                PRECIP_KINDS[kind],
                &tx("pause.world.text.rain_or_snow"),
                None,
            ),
            "precipkind".to_string(),
        ));
        weather.extend(slider_row(
            Some(app),
            "rain_amt",
            &tx("pause.world.text.precipitation_strength"),
            &tx("pause.world.text.how_hard_it_rains_or_snows"),
            &pct,
        ));
        weather.extend(slider_row(
            Some(app),
            "wet",
            &tx("pause.world.text.wet_roads"),
            &tx("pause.world.text.how_wet_the_roads_are_now_they_dry_in_the_sun_wet_in_the_rain"),
            &pct,
        ));
        weather.extend(switch_row(
            Some(app),
            "snow_cover",
            &tx("pause.world.text.snow_cover"),
            &tx("pause.world.text.snow_lying_on_the_world_and_ground"),
        ));
        weather.extend(switch_row(
            Some(app),
            "snow_road",
            &tx("pause.world.text.snow_on_road"),
            &tx("pause.world.text.treat_the_road_surface_as_snow_covered"),
        ));
        climate.extend(slider_row(
            Some(app),
            "temp",
            &tx("pause.world.text.temperature"),
            &tx("pause.world.text.the_air_temperature"),
            &|v| format!("{} °C", v as i64),
        ));
        let dew_temp = app.weather.as_ref().map(|w| w.temp.0).unwrap_or(15.0);
        climate.extend(slider_row(
            Some(app),
            "humidity",
            &tx("pause.world.text.humidity"),
            &tx("pause.world.text.relative_humidity_of_the_air"),
            &|v| {
                format!(
                    "{:.0} % · {} {:.0} °C",
                    v,
                    tx("pause.world.dew"),
                    crate::weather_setup::dew_point_c(dew_temp, v)
                )
            },
        ));
        climate.extend(slider_row(
            Some(app),
            "wind_speed",
            &tx("pause.world.text.wind_speed"),
            &tx("pause.world.text.how_fast_the_wind_blows_it_drives_the_clouds"),
            &|v| format!("{} m/s", v as i64),
        ));
        climate.extend(slider_row(
            Some(app),
            "wind_dir",
            &tx("pause.world.text.wind_direction"),
            &tx("pause.world.text.the_direction_of_the_wind_in_degrees_0_is_north"),
            &|v| format!("{}°", v as i64),
        ));
        // the METAR sync on: only its own rows stay (the weather is the report's)
        if app.metar_locked() {
            weather.retain(|r| {
                matches!(
                    r.1.as_str(),
                    "metar_sync" | "metar_src" | "metar_icao_edit" | "metar_refresh"
                )
            });
            climate.clear();
        }
        tools.push(button(
            &tx("pause.world.text.object_editor"),
            &tx("pause.world.text.open"),
            &tx("pause.world.text.place_and_move_objects_in_the_world"),
            "editor",
        ));
    }
    let mut people: Vec<(String, String)> = Vec::new();
    people.extend(slider_row(
        Some(app),
        "traffic",
        &tx("pause.world.text.traffic"),
        &tx("pause.world.text.how_many_vehicles_drive_around_the_map"),
        &|v| format!("{} vehicles", v as i64),
    ));
    people.extend(slider_row(
        Some(app),
        "pax",
        &tx("pause.world.text.passengers"),
        &tx("pause.world.text.how_many_passengers_wait_at_the_stops_and_ride"),
        &pct,
    ));
    vec![
        (tx("pause.world.text.time"), time),
        (tx("pause.world.text.weather"), weather),
        (tx("pause.world.text.temperature_and_wind"), climate),
        (tx("pause.world.text.traffic_and_people"), people),
        (tx("pause.world.text.tools"), tools),
    ]
}
