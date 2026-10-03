//! The Drive page: the duty put together in four steps - the bus, the route (map, line,
//! tour), the time and weather, the roadbook - in a panel on the left, the bus itself in
//! the showroom on the right, and the button that starts the game.

use super::Launcher;
use super::state::{fmt_bytes, hhmm, trip_index_at};
use super::theme::*;
use super::ui::{ButtonKind, id_of};
use glam::Vec2;
use omsi_launcher_lib::{display_bus_name, vehicle_type_label};
use omsi_ui::paint::Align;
use omsi_ui::{Color, Rect, Weight};

#[derive(Clone)]
struct BusVariant {
    file: String,
    name: String,
    variant: String,
    fresh: bool,
    installed: bool,
    paints: usize,
    incomplete: bool,
}

#[derive(Clone)]
struct BusManufacturer {
    key: String,
    name: String,
    variants: Vec<BusVariant>,
}

#[derive(Default)]
pub struct DriveView {
    pub step: usize,
    pub bus_filter: String,
    /// The original OMSI manufacturer/type hierarchy, cached between content updates.
    bus_manufacturers: std::sync::Arc<Vec<BusManufacturer>>,
    bus_manufacturers_key: (usize, u64, usize),
    expanded_manufacturer: Option<String>,
    bus_list_initialized: bool,
    vehicle_settings_open: bool,
    pub line_filter: String,
    /// The buses marked with a star (#524), by file (lower case, '/'), read once from
    /// `~/.neoomsi/favourite-buses.txt`; and whether the list shows only them.
    favourites: Option<std::collections::BTreeSet<String>>,
    only_favourites: bool,
}

fn favourites_file() -> std::path::PathBuf {
    omsi_launcher_lib::data_dir().join("favourite-buses.txt")
}

fn fav_key(file: &str) -> String {
    file.replace('\\', "/").to_lowercase()
}

/// The starred buses of the file (one bus file a line).
fn read_favourites() -> std::collections::BTreeSet<String> {
    std::fs::read_to_string(favourites_file())
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(fav_key)
                .collect()
        })
        .unwrap_or_default()
}

fn write_favourites(f: &std::collections::BTreeSet<String>) {
    let text: String = f.iter().map(|l| format!("{l}\n")).collect();
    if let Err(e) = std::fs::write(favourites_file(), text) {
        log::warn!("favourite buses not saved: {e}");
    }
}

const STEPS: [(&str, &str); 4] = [
    ("Bus", "directions_bus"),
    ("Route", "route"),
    ("Time & weather", "partly_cloudy_day"),
    ("Roadbook", "receipt_long"),
];

pub fn draw(l: &mut Launcher, area: Rect) {
    let form_w = (area.w * 0.5).clamp(440.0, 580.0);
    let panel = Rect::new(area.x, area.y, form_w, area.h);
    l.ui.panel(panel);
    // the steps: plain tabs with a line under the chosen one
    let tabs = Rect::new(panel.x + 16.0, panel.y + 8.0, panel.w - 32.0, 42.0);
    let w = tabs.w / 4.0;
    let sel = l.ui.anim(id_of("drive-step"), l.drive.step as f32, 0.08);
    l.ui.p()
        .rect(Rect::new(tabs.x, tabs.bottom() - 1.0, tabs.w, 1.0), EDGE);
    l.ui.p().rect(
        Rect::new(tabs.x + w * sel, tabs.bottom() - 2.0, w, 2.0),
        ACCENT,
    );
    for (k, (name, _)) in STEPS.iter().enumerate() {
        let cell = Rect::new(tabs.x + w * k as f32, tabs.y, w, tabs.h - 2.0);
        let id = id_of(&format!("step-{k}"));
        let (h, _, clicked) = l.ui.interact(id, cell);
        if clicked {
            l.drive.step = k;
            if k == 3 {
                l.state.load_ibis();
            }
        }
        let on = l.drive.step == k;
        let c = if on {
            TEXT
        } else if h {
            TEXT_SOFT
        } else {
            TEXT_DIM
        };
        l.ui.text_in(
            name,
            cell,
            13.0,
            if on { Weight::Medium } else { Weight::Regular },
            c,
            Align::Center,
        );
    }
    let body = Rect::new(
        panel.x + 18.0,
        tabs.bottom() + 16.0,
        panel.w - 36.0,
        panel.bottom() - tabs.bottom() - 32.0,
    );
    match l.drive.step {
        0 => step_bus(l, body),
        1 => step_route(l, body),
        2 => step_time(l, body),
        _ => step_roadbook(l, body),
    }
    // the right side: the bus, what the duty is, and the start
    let side = Rect::new(
        panel.right() + 24.0,
        area.y,
        area.right() - panel.right() - 24.0,
        area.h,
    );
    summary(l, side);
}

/// OMSI takes the manufacturer and the complete type from [friendlyname]. The
/// vehicle folder and rendering configuration do not define this hierarchy.
fn build_bus_manufacturers(
    vehicles: &[omsi_launcher_lib::VehicleInfo],
    allowed: Option<&std::collections::HashSet<String>>,
    fresh: &std::collections::HashSet<String>,
) -> Vec<BusManufacturer> {
    let mut grouped = std::collections::BTreeMap::<String, BusManufacturer>::new();
    for vehicle in vehicles {
        if !allowed
            .map(|a| a.contains(&vehicle.file.replace('\\', "/").to_lowercase()))
            .unwrap_or(true)
        {
            continue;
        }
        let maker = vehicle.manufacturer.trim();
        let key = maker.to_lowercase();
        let group = grouped
            .entry(key.clone())
            .or_insert_with(|| BusManufacturer {
                key,
                name: if maker.is_empty() {
                    "Unknown manufacturer".into()
                } else {
                    display_bus_name(maker)
                },
                variants: Vec::new(),
            });
        let type_name = vehicle_type_label(&vehicle.type_name, std::path::Path::new(&vehicle.file));
        group.variants.push(BusVariant {
            file: vehicle.file.clone(),
            name: display_bus_name(&vehicle.name),
            variant: type_name,
            fresh: fresh.contains(&vehicle.file),
            installed: vehicle.installed,
            paints: vehicle.paints.len(),
            incomplete: !vehicle.missing_packs.is_empty(),
        });
    }
    let mut manufacturers: Vec<BusManufacturer> = grouped.into_values().collect();
    for maker in &mut manufacturers {
        // Distinct .bus files remain selectable even when add-ons repeat a friendly
        // type name. Show the pack, and the file only if the pack also repeats it.
        let mut counts = std::collections::HashMap::<String, usize>::new();
        for variant in &maker.variants {
            *counts.entry(variant.variant.to_lowercase()).or_default() += 1;
        }
        for variant in &mut maker.variants {
            if counts[&variant.variant.to_lowercase()] > 1 {
                let folder = variant
                    .file
                    .replace('\\', "/")
                    .split('/')
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                variant.variant = format!("{} · {}", variant.variant, display_bus_name(&folder));
            }
        }
        let mut counts = std::collections::HashMap::<String, usize>::new();
        for variant in &maker.variants {
            *counts.entry(variant.variant.to_lowercase()).or_default() += 1;
        }
        for variant in &mut maker.variants {
            if counts[&variant.variant.to_lowercase()] > 1 {
                let stem = std::path::Path::new(&variant.file)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                variant.variant = format!("{} · {}", variant.variant, display_bus_name(&stem));
            }
        }
        maker
            .variants
            .sort_by(|a, b| bus_name_cmp(&a.variant, &b.variant).then_with(|| a.file.cmp(&b.file)));
    }
    manufacturers.sort_by(|a, b| bus_name_cmp(&a.name, &b.name).then_with(|| a.key.cmp(&b.key)));
    manufacturers
}

/// Sort numeric runs wherever they occur: DL9 precedes DL10; case does not change
/// a manufacturer's position.
fn bus_name_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let a = a.to_lowercase();
    let b = b.to_lowercase();
    let mut a = a.chars().peekable();
    let mut b = b.chars().peekable();
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let x: String = std::iter::from_fn(|| a.next_if(|c| c.is_ascii_digit())).collect();
                let y: String = std::iter::from_fn(|| b.next_if(|c| c.is_ascii_digit())).collect();
                let x = x.trim_start_matches('0');
                let y = y.trim_start_matches('0');
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.cmp(&y);
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

pub(super) fn default_livery_label(vehicle: &omsi_launcher_lib::VehicleInfo) -> &str {
    if vehicle.default_paint.trim().is_empty() {
        "Default paint"
    } else {
        vehicle.default_paint.trim()
    }
}

fn variant_matches(variant: &BusVariant, q: &str) -> bool {
    q.is_empty()
        || variant.name.to_lowercase().contains(q)
        || variant.variant.to_lowercase().contains(q)
        || display_bus_name(&variant.file).to_lowercase().contains(q)
}

fn manufacturer_matches(model: &BusManufacturer, q: &str) -> bool {
    q.is_empty()
        || model.name.to_lowercase().contains(q)
        || model.variants.iter().any(|v| variant_matches(v, q))
}

fn step_bus(l: &mut Launcher, r: Rect) {
    l.ui.heading(Rect::new(r.x, r.y, r.w, 24.0), "Choose a bus", None);
    let search = Rect::new(r.x, r.y + 32.0, r.w, ROW);
    let search_changed = l.ui.text_input(
        "bus-filter",
        search,
        &mut l.drive.bus_filter,
        "Search buses…",
        Some("search"),
    );
    if search_changed {
        l.ui.scroll.remove(&id_of("bus-model-list"));
        l.ui.scroll.remove(&(id_of("bus-model-list") ^ 0xabc));
    }
    let q = display_bus_name(l.drive.bus_filter.trim()).to_lowercase();
    let norm = |f: &str| f.replace('\\', "/").to_lowercase();
    let allowed: Option<std::collections::HashSet<String>> = l
        .state
        .host_vehicles()
        .map(|v| v.iter().map(|f| norm(f)).collect());
    if let Some(a) = allowed.as_ref() {
        if !a.contains(&norm(&l.state.choice.bus)) {
            if let Some(first) = l
                .state
                .vehicles
                .iter()
                .find(|v| a.contains(&norm(&v.file)))
                .map(|v| v.file.clone())
            {
                l.state.select_bus(&first);
            }
        }
    }
    let allowed_key = allowed
        .as_ref()
        .map(|a| a.iter().fold(0u64, |hash, file| hash ^ id_of(file)))
        .unwrap_or(u64::MAX);
    let key = (l.state.vehicles.len(), allowed_key, l.state.fresh.len());
    if key != l.drive.bus_manufacturers_key
        || (l.drive.bus_manufacturers.is_empty() && !l.state.vehicles.is_empty())
    {
        let fresh = l.state.fresh.keys().cloned().collect();
        l.drive.bus_manufacturers = std::sync::Arc::new(build_bus_manufacturers(
            &l.state.vehicles,
            allowed.as_ref(),
            &fresh,
        ));
        l.drive.bus_manufacturers_key = key;
    }
    let models = l.drive.bus_manufacturers.clone();
    let chosen = l.state.choice.bus.clone();
    let favs = l
        .drive
        .favourites
        .get_or_insert_with(read_favourites)
        .clone();
    let is_fav = |file: &str| favs.contains(&fav_key(file));
    // (the starred ones only: a family with one of them, and in it only those - #524)
    let only = l.drive.only_favourites && !favs.is_empty();

    let visible: Vec<&BusManufacturer> = models
        .iter()
        .filter(|m| {
            manufacturer_matches(m, &q) && (!only || m.variants.iter().any(|v| is_fav(&v.file)))
        })
        .collect();
    let count = format!(
        "{} {}",
        visible.len(),
        omsi_ui::tr(if visible.len() == 1 {
            "manufacturer"
        } else {
            "manufacturers"
        })
    );
    l.ui.text_in(
        &count,
        Rect::new(r.x, search.bottom() + 6.0, r.w, 20.0),
        11.5,
        Weight::Regular,
        TEXT_DIM,
        Align::Left,
    );
    let mut only_now = l.drive.only_favourites;
    if l.ui.toggle(
        "bus-only-favourites",
        Rect::new(r.right() - 190.0, search.bottom() + 4.0, 190.0, 22.0),
        &mut only_now,
        "Favourites only",
    ) {
        l.drive.only_favourites = only_now;
        l.ui.scroll.remove(&id_of("bus-model-list"));
    }
    let list_y = search.bottom() + 32.0;
    let settings_h = if l.drive.vehicle_settings_open {
        164.0
    } else {
        0.0
    };
    let list = Rect::new(
        r.x,
        list_y,
        r.w,
        (r.bottom() - list_y - 146.0 - settings_h).max(100.0),
    );
    l.ui.p().rounded(list, RADIUS, FIELD);
    l.ui.p().rounded_border(list, RADIUS, 1.0, EDGE);
    if !l.drive.bus_list_initialized && !models.is_empty() {
        if let Some(index) = models
            .iter()
            .position(|m| m.variants.iter().any(|v| v.file == chosen))
        {
            let model = &models[index];
            l.drive.expanded_manufacturer = Some(model.key.clone());
            let selected_y = 6.0 + index as f32 * 58.0;
            l.ui.scroll_to(
                "bus-model-list",
                selected_y,
                if model.variants.len() > 1 {
                    120.0
                } else {
                    54.0
                },
                list.h,
            );
        }
        l.drive.bus_list_initialized = true;
    }
    if search_changed && !q.is_empty() {
        l.drive.expanded_manufacturer = visible.first().map(|m| m.key.clone());
    }
    let expanded = l.drive.expanded_manufacturer.clone();
    let mut toggle = None;
    let mut pick = None;
    let mut star: Option<String> = None;
    let loading = l.state.loading_content;
    l.ui.scroll_area("bus-model-list", list, &mut |ui, view| {
        let mut y = view.y + 6.0;
        if visible.is_empty() {
            ui.text_in(
                if loading {
                    "Reading the buses…"
                } else {
                    "No buses found. Try another search."
                },
                Rect::new(view.x + 12.0, y, view.w - 24.0, 50.0),
                12.5,
                Weight::Regular,
                TEXT_DIM,
                Align::Left,
            );
        }
        for model in &visible {
            let open = model.variants.len() > 1 && expanded.as_deref() == Some(model.key.as_str());
            let row = Rect::new(view.x + 6.0, y, view.w - 18.0, 54.0);
            // Keep the scroll extent, but avoid shaping text and emitting geometry for
            // invisible families. The expanded dropdown retains its widget state.
            if !open && !ui.rect_visible(row) {
                y += 58.0;
                continue;
            }
            let selected = model.variants.iter().find(|v| v.file == chosen);
            if ui.row(
                &format!("bus-family-{}", model.key),
                row,
                selected.is_some(),
            ) {
                if model.variants.len() == 1 {
                    pick = Some(model.variants[0].file.clone());
                } else {
                    toggle = Some(model.key.clone());
                }
            }
            ui.icon(
                "directions_bus",
                Vec2::new(row.x + 20.0, row.y + 23.0),
                20.0,
                if selected.is_some() { ACCENT } else { TEXT_DIM },
            );
            let title = Rect::new(row.x + 42.0, row.y + 7.0, row.w - 78.0, 20.0);
            ui.text_in(&model.name, title, 13.0, Weight::Medium, TEXT, Align::Left);
            ui.tooltip(title, &model.name);
            let subtitle = if model.variants.len() == 1 {
                omsi_ui::tr(&model.variants[0].variant).into_owned()
            } else if let Some(v) = selected {
                format!(
                    "{} · {} {}",
                    omsi_ui::tr(&v.variant),
                    model.variants.len(),
                    omsi_ui::tr("models")
                )
            } else {
                format!("{} {}", model.variants.len(), omsi_ui::tr("models"))
            };
            let subtitle = if selected.is_some_and(|v| v.incomplete) {
                format!("{subtitle} · {}", omsi_ui::tr("PARTS MISSING"))
            } else if selected.is_some_and(|v| v.fresh) {
                format!("{subtitle} · {}", omsi_ui::tr("NEW"))
            } else if selected.is_some_and(|v| v.installed) {
                format!("{subtitle} · {}", omsi_ui::tr("MOD"))
            } else {
                subtitle
            };
            ui.text_in(
                &subtitle,
                Rect::new(title.x, row.y + 29.0, title.w, 17.0),
                11.5,
                Weight::Regular,
                TEXT_DIM,
                Align::Left,
            );
            ui.icon(
                if model.variants.len() == 1 {
                    if selected.is_some() {
                        "check"
                    } else {
                        "chevron_right"
                    }
                } else if open {
                    "expand_less"
                } else {
                    "expand_more"
                },
                Vec2::new(row.right() - 18.0, row.center().y),
                18.0,
                if selected.is_some() { ACCENT } else { TEXT_DIM },
            );
            // the star: a bus of its own is starred here, a family's types in its list below
            let starred = model.variants.iter().any(|v| is_fav(&v.file));
            let sr = Rect::new(row.right() - 62.0, row.center().y - 13.0, 26.0, 26.0);
            if model.variants.len() == 1 {
                let (hs, _, cs) = ui.interact(id_of(&format!("bus-star-{}", model.key)), sr);
                if cs {
                    star = Some(model.variants[0].file.clone());
                }
                ui.icon(
                    "star",
                    sr.center(),
                    16.0,
                    if starred {
                        ACCENT
                    } else if hs {
                        TEXT_SOFT
                    } else {
                        Color::WHITE.alpha(0.16)
                    },
                );
                ui.tooltip(
                    sr,
                    if starred {
                        "Remove from the favourites"
                    } else {
                        "Add to the favourites"
                    },
                );
            } else if starred {
                ui.icon("star", sr.center(), 14.0, ACCENT.alpha(0.8));
            }
            y += 58.0;
            if open {
                let variants: Vec<&BusVariant> = model
                    .variants
                    .iter()
                    .filter(|variant| {
                        (q.is_empty()
                            || model.name.to_lowercase().contains(&q)
                            || variant.file == chosen
                            || variant_matches(variant, &q))
                            && (!only || variant.file == chosen || is_fav(&variant.file))
                    })
                    .collect();
                let selected_index = variants.iter().position(|variant| variant.file == chosen);
                let mut options: Vec<String> = variants
                    .iter()
                    .map(|variant| variant.variant.clone())
                    .collect();
                let offset = if selected_index.is_none() {
                    options.insert(0, "Choose a bus".into());
                    1
                } else {
                    0
                };
                let mut sel = selected_index.unwrap_or(0);
                ui.label(
                    Rect::new(view.x + 38.0, y, view.w - 50.0, 22.0),
                    "Type / variant",
                );
                y += 26.0;
                let dropdown = Rect::new(view.x + 38.0, y, view.w - 50.0 - 36.0, ROW);
                if !options.is_empty()
                    && ui.select(
                        &format!("bus-type-{}", model.key),
                        dropdown,
                        &mut sel,
                        &options,
                    )
                {
                    if let Some(variant) = sel
                        .checked_sub(offset)
                        .and_then(|index| variants.get(index))
                    {
                        pick = Some(variant.file.clone());
                    }
                }
                // the star of the type chosen
                if let Some(variant) = selected {
                    let sr = Rect::new(dropdown.right() + 6.0, y, 30.0, ROW);
                    let on = is_fav(&variant.file);
                    let (hs, _, cs) =
                        ui.interact(id_of(&format!("bus-star-type-{}", model.key)), sr);
                    if cs {
                        star = Some(variant.file.clone());
                    }
                    ui.icon(
                        "star",
                        sr.center(),
                        18.0,
                        if on {
                            ACCENT
                        } else if hs {
                            TEXT_SOFT
                        } else {
                            Color::WHITE.alpha(0.2)
                        },
                    );
                    ui.tooltip(
                        sr,
                        if on {
                            "Remove from the favourites"
                        } else {
                            "Add to the favourites"
                        },
                    );
                }
                if let Some(variant) = selected {
                    ui.tooltip(
                        dropdown,
                        &format!(
                            "{}\n{}\n{} {}",
                            variant.name,
                            variant.file,
                            variant.paints,
                            omsi_ui::tr("liveries")
                        ),
                    );
                }
                y += ROW + 10.0;
            }
        }
        y - view.y + 4.0
    });
    if let Some(key) = toggle {
        l.drive.expanded_manufacturer = if l.drive.expanded_manufacturer.as_ref() == Some(&key) {
            None
        } else {
            Some(key.clone())
        };
        if l.drive.expanded_manufacturer.is_some() {
            if let Some(index) = visible.iter().position(|maker| maker.key == key) {
                l.ui.scroll_to("bus-model-list", 6.0 + index as f32 * 58.0, 120.0, list.h);
            }
        }
    }
    if let Some(file) = pick {
        l.state.select_bus(&file);
    }
    if let Some(file) = star {
        let f = l.drive.favourites.get_or_insert_with(read_favourites);
        let k = fav_key(&file);
        if !f.remove(&k) {
            f.insert(k);
        }
        write_favourites(f);
    }

    let mut y = list.bottom() + 16.0;
    if let Some(vehicle) = l.state.bus().cloned() {
        let paints: Vec<String> = std::iter::once(default_livery_label(&vehicle).to_string())
            .chain(vehicle.paints.iter().cloned())
            .collect();
        let mut paint_sel = vehicle
            .paints
            .iter()
            .position(|p| *p == l.state.choice.paint)
            .map(|i| i + 1)
            .unwrap_or(0);
        l.ui.label(Rect::new(r.x, y, r.w, 22.0), "Livery");
        if paints.len() > 1 {
            l.ui.text_in(
                &format!("{} / {}", paint_sel + 1, paints.len()),
                Rect::new(r.right() - 70.0, y, 70.0, 22.0),
                11.5,
                Weight::Regular,
                TEXT_DIM,
                Align::Right,
            );
        }
        y += 28.0;
        let has_arrows = paints.len() > 1;
        let selector = Rect::new(r.x, y, r.w - if has_arrows { 88.0 } else { 0.0 }, ROW);
        let mut changed = l.ui.select("paint", selector, &mut paint_sel, &paints);
        if has_arrows {
            let prev = Rect::new(selector.right() + 8.0, y, ROW, ROW);
            let next = Rect::new(prev.right() + 8.0, y, ROW, ROW);
            if l.ui.button(
                "paint-previous",
                prev,
                "",
                Some("chevron_left"),
                ButtonKind::Normal,
            ) {
                paint_sel = (paint_sel + paints.len() - 1) % paints.len();
                changed = true;
            }
            if l.ui.button(
                "paint-next",
                next,
                "",
                Some("chevron_right"),
                ButtonKind::Normal,
            ) {
                paint_sel = (paint_sel + 1) % paints.len();
                changed = true;
            }
            l.ui.tooltip(prev, "Preview previous livery");
            l.ui.tooltip(next, "Preview next livery");
        }
        if changed {
            l.state.choice.paint = if paint_sel == 0 {
                String::new()
            } else {
                vehicle.paints[paint_sel - 1].clone()
            };
            l.state.touched();
        }
        y += ROW + 14.0;
        let settings = Rect::new(r.x, y, r.w, 32.0);
        if l.ui.row("vehicle-settings-toggle", settings, false) {
            l.drive.vehicle_settings_open = !l.drive.vehicle_settings_open;
        }
        l.ui.icon(
            if l.drive.vehicle_settings_open {
                "expand_less"
            } else {
                "expand_more"
            },
            Vec2::new(settings.x + 12.0, settings.center().y),
            18.0,
            TEXT_DIM,
        );
        l.ui.text_in(
            "Vehicle settings & details",
            Rect::new(settings.x + 30.0, settings.y, settings.w - 30.0, settings.h),
            12.5,
            Weight::Medium,
            TEXT_DIM,
            Align::Left,
        );
        y += 38.0;
        if l.drive.vehicle_settings_open {
            let details = Rect::new(r.x, y, r.w, (r.bottom() - y).max(0.0));
            let mut hof_pick = None;
            let mut number_pick = None;
            let number_options: Vec<String> = vehicle
                .numbers
                .iter()
                .map(|(number, plate)| {
                    if plate.trim().is_empty() {
                        number.clone()
                    } else {
                        format!("{number}  ({})", plate.trim())
                    }
                })
                .collect();
            let mut number_sel = vehicle
                .numbers
                .iter()
                .position(|(number, _)| *number == l.state.choice.number)
                .unwrap_or(0);
            let mut plate = l.state.choice.plate.clone();
            let auto = l.state.default_hof();
            let mut hof_options = vec![format!("Automatic ({auto})")];
            hof_options.extend(vehicle.hofs.iter().cloned());
            let mut hof_sel = if l.state.choice.hof_manual {
                vehicle
                    .hofs
                    .iter()
                    .position(|h| h.eq_ignore_ascii_case(&l.state.choice.hof))
                    .map(|i| i + 1)
                    .unwrap_or(0)
            } else {
                0
            };
            let mut plate_changed = false;
            l.ui.scroll_area("bus-details", details, &mut |ui, view| {
                let mut y = view.y;
                let field_w = view.w - 8.0;
                ui.label(Rect::new(view.x, y, 110.0, ROW), "Depot file");
                if ui.select(
                    "hof",
                    Rect::new(view.x + 110.0, y, field_w - 110.0, ROW),
                    &mut hof_sel,
                    &hof_options,
                ) {
                    hof_pick = Some(hof_sel);
                }
                y += ROW + 8.0;
                if !number_options.is_empty() {
                    ui.label(Rect::new(view.x, y, 110.0, ROW), "Fleet number");
                    if ui.select(
                        "number",
                        Rect::new(view.x + 110.0, y, field_w - 110.0, ROW),
                        &mut number_sel,
                        &number_options,
                    ) {
                        number_pick = Some(number_sel);
                    }
                    y += ROW + 8.0;
                }
                ui.label(Rect::new(view.x, y, 110.0, ROW), "Number plate");
                plate_changed = ui.text_input(
                    "plate",
                    Rect::new(view.x + 110.0, y, field_w - 110.0, ROW),
                    &mut plate,
                    "Automatic",
                    Some("badge"),
                );
                y += ROW + 16.0;
                if !vehicle.missing_packs.is_empty() {
                    y += ui.paragraph(
                        &omsi_ui::tr("Parts missing: needs %{packs}")
                            .replace("%{packs}", &vehicle.missing_packs.join(", ")),
                        Vec2::new(view.x, y),
                        field_w,
                        12.5,
                        Weight::Regular,
                        WARN,
                    ) + 12.0;
                }
                let description = vehicle
                    .description
                    .replace('\t', " ")
                    .lines()
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string();
                if !description.is_empty() {
                    y += ui.paragraph(
                        &description,
                        Vec2::new(view.x, y),
                        field_w,
                        12.0,
                        Weight::Regular,
                        TEXT_DIM,
                    ) + 12.0;
                }
                y += ui.paragraph(
                    &vehicle.file,
                    Vec2::new(view.x, y),
                    field_w,
                    10.5,
                    Weight::Regular,
                    TEXT_FAINT,
                );
                y - view.y + 8.0
            });
            if let Some(sel) = hof_pick {
                l.state.choice.hof_manual = sel != 0;
                l.state.choice.hof = if sel == 0 {
                    auto
                } else {
                    vehicle.hofs[sel - 1].clone()
                };
                l.state.touched();
            }
            if let Some(sel) = number_pick {
                l.state.choice.number = vehicle.numbers[sel].0.clone();
                l.state.touched();
            }
            if plate_changed {
                l.state.choice.plate = plate;
                l.state.touched();
            }
        }
    }
}

fn step_route(l: &mut Launcher, r: Rect) {
    let mut y = r.y;
    // the map (on a server: the server's, not to be changed)
    if let Some(name) = joined_server_name(l) {
        let m = l
            .state
            .map()
            .map(|m| {
                if m.friendly.is_empty() {
                    m.name.clone()
                } else {
                    m.friendly.clone()
                }
            })
            .unwrap_or_else(|| l.state.choice.map.clone());
        l.ui.label(Rect::new(r.x, y, 110.0, ROW), "Map");
        let fr = Rect::new(r.x + 110.0, y, r.w - 110.0, ROW);
        l.ui.solid(fr);
        l.ui.p().rounded(fr, RADIUS, FIELD);
        l.ui.icon(
            "lock",
            Vec2::new(fr.x + 16.0, fr.center().y),
            15.0,
            TEXT_DIM,
        );
        l.ui.text_in(
            &format!("{m}  ·  set by {name}"),
            Rect::new(fr.x + 32.0, fr.y, fr.w - 40.0, fr.h),
            13.0,
            Weight::Regular,
            TEXT_SOFT,
            Align::Left,
        );
        y += ROW + 8.0;
        return step_route_rest(l, r, y);
    }
    let maps: Vec<(String, String)> = l
        .state
        .maps
        .iter()
        .map(|m| {
            (
                m.file.clone(),
                format!(
                    "{}{}{}",
                    if l.state.fresh.contains_key(&m.file) {
                        "★ NEW · "
                    } else {
                        ""
                    },
                    if m.friendly.is_empty() {
                        &m.name
                    } else {
                        &m.friendly
                    },
                    if m.installed { "  (mod)" } else { "" }
                ),
            )
        })
        .collect();
    let mut sel = maps
        .iter()
        .position(|m| m.0 == l.state.choice.map)
        .unwrap_or(0);
    l.ui.label(Rect::new(r.x, y, 110.0, ROW), "Map");
    if l.ui.select(
        "map",
        Rect::new(r.x + 110.0, y, r.w - 110.0, ROW),
        &mut sel,
        &maps.iter().map(|m| m.1.clone()).collect::<Vec<_>>(),
    ) {
        if let Some((f, _)) = maps.get(sel) {
            let f = f.clone();
            l.state.select_map(&f);
        }
    }
    y += ROW + 8.0;
    step_route_rest(l, r, y);
}

fn step_route_rest(l: &mut Launcher, r: Rect, mut y: f32) {
    let mut free = l.state.choice.free;
    if l.ui.toggle(
        "free",
        Rect::new(r.x, y, r.w, ROW),
        &mut free,
        "Free drive (no timetable duty)",
    ) {
        l.state.choice.free = free;
        l.state.touched();
    }
    y += ROW + 10.0;
    // where to start: one of the map's entry points, as in OMSI 2; Automatic takes the one
    // nearest to the duty's first stop by road (a free drive: the map's first)
    if let Some(m) = l.state.map().cloned() {
        let mut labels = vec![if free {
            "Automatic (the map's first)".to_string()
        } else {
            "Automatic (nearest to the first stop)".to_string()
        }];
        labels.extend(m.entry_points.iter().map(|e| {
            if e.name.is_empty() {
                format!("entry {}", e.index + 1)
            } else {
                e.name.clone()
            }
        }));
        // (the choice is the entry's place in the list; 0 = automatic here)
        let mut es = if l.state.choice.entry < 0 {
            0
        } else {
            (l.state.choice.entry as usize + 1).min(labels.len() - 1)
        };
        l.ui.label(Rect::new(r.x, y, 110.0, ROW), "Start at");
        if l.ui.select(
            "entry",
            Rect::new(r.x + 110.0, y, r.w - 110.0, ROW),
            &mut es,
            &labels,
        ) {
            l.state.choice.entry = es as i32 - 1;
            l.state.touched();
        }
        y += ROW + 10.0;
    }
    if free {
        l.ui.paragraph("Free driving: the bus starts where you chose, with the traffic and the timetable's buses around it, but no line of your own.", Vec2::new(r.x, y), r.w, 13.0, Weight::Regular, TEXT_DIM);
        return;
    }
    // lines and tours side by side
    let half = (r.w - 12.0) * 0.5;
    let left = Rect::new(r.x, y, half, r.bottom() - y);
    let right = Rect::new(r.x + half + 12.0, y, half, r.bottom() - y);
    l.ui.heading(Rect::new(left.x, left.y, left.w, 28.0), "Line", None);
    l.ui.text_input(
        "line-filter",
        Rect::new(left.x, left.y + 30.0, left.w, 34.0),
        &mut l.drive.line_filter,
        "Filter…",
        Some("search"),
    );
    let q = l.drive.line_filter.to_lowercase();
    let lines: Vec<(String, String, usize)> = l
        .state
        .lines
        .iter()
        .filter(|x| x.user_allowed)
        .filter(|x| q.is_empty() || x.name.to_lowercase().contains(&q))
        .map(|x| (x.name.clone(), x.termini.join(" · "), x.tours.len()))
        .collect();
    let chosen = l.state.choice.line.clone();
    let mut pick_line = None;
    let loading = l.state.loading_lines;
    l.ui.scroll_area(
        "line-list",
        Rect::new(left.x - 4.0, left.y + 72.0, left.w + 8.0, left.h - 72.0),
        &mut |ui, v| {
            let rh = 48.0;
            if lines.is_empty() {
                ui.text_in(
                    if loading {
                        "Reading the timetable…"
                    } else {
                        "No lines on this date."
                    },
                    Rect::new(v.x + 10.0, v.y, v.w, 36.0),
                    12.5,
                    Weight::Regular,
                    TEXT_DIM,
                    Align::Left,
                );
            }
            for (k, (name, termini, tours)) in lines.iter().enumerate() {
                let rr = Rect::new(v.x + 4.0, v.y + k as f32 * rh, v.w - 12.0, rh - 4.0);
                let on = chosen.as_deref() == Some(name.as_str());
                if ui.row(&format!("line-{name}"), rr, on) {
                    pick_line = Some(name.clone());
                }
                // (a long line name, as Ahlheim's "Eichenhoehe TA11 Mo-Do Schule", is cut)
                let count = format!("{tours}");
                let cw = ui.width(&count, 11.5, Weight::Bold) + 8.0;
                let bw = (ui.width(name, 13.0, Weight::Black) + 14.0)
                    .clamp(34.0, (rr.w - cw - 24.0).max(34.0));
                let badge = Rect::new(rr.x + 8.0, rr.y + 8.0, bw, 22.0);
                ui.p().rounded(badge, 4.0, Color::rgba(52, 52, 52, 1.0));
                ui.text_in(
                    name,
                    badge.pad(6.0, 0.0),
                    12.5,
                    Weight::Bold,
                    TEXT,
                    Align::Center,
                );
                ui.icon(
                    "event",
                    Vec2::new(rr.right() - cw - 8.0, rr.y + 19.0),
                    13.0,
                    TEXT_FAINT,
                );
                ui.text_in(
                    &count,
                    Rect::new(rr.right() - cw, rr.y + 8.0, cw - 4.0, 22.0),
                    11.5,
                    Weight::Bold,
                    TEXT_DIM,
                    Align::Right,
                );
                ui.text_in(
                    termini,
                    Rect::new(rr.x + 8.0, rr.y + 28.0, rr.w - 16.0, 16.0),
                    11.0,
                    Weight::Regular,
                    TEXT_FAINT,
                    Align::Left,
                );
            }
            lines.len() as f32 * rh + 4.0
        },
    );
    if let Some(n) = pick_line {
        if l.state.choice.line.as_deref() != Some(n.as_str()) {
            l.state.choice.line = Some(n);
            l.state.choice.tour = None;
            l.state.touched();
        }
    }
    l.ui.heading(Rect::new(right.x, right.y, right.w, 28.0), "Tour", None);
    let Some(line) = l.state.line().cloned() else {
        l.ui.paragraph("Pick a line first. As in OMSI, the start time and date then say where in the tour the bus is: the trip under way, or the next to leave.", Vec2::new(right.x, right.y + 34.0), right.w, 12.5, Weight::Regular, TEXT_DIM);
        return;
    };
    let mut tours: Vec<&omsi_launcher_lib::TourInfo> = line.tours.iter().collect();
    tours.sort_by(|a, b| {
        b.runs
            .cmp(&a.runs)
            .then_with(|| natural(&a.number).cmp(&natural(&b.number)))
    });
    let now = l.state.choice.time as f64 * 60.0;
    let picked = l.state.choice.tour.clone().zip(l.state.first_trip());
    let tours: Vec<(
        String,
        usize,
        String,
        bool,
        Option<String>,
        Option<(f64, f64)>,
        String,
        String,
    )> = tours
        .iter()
        .map(|t| {
            let first = match &picked {
                Some((n, k)) if *n == t.number => Some(*k),
                _ => trip_index_at(t, now),
            };
            let trip = first.and_then(|i| t.trips.get(i));
            let from = trip.map(|x| x.from.clone()).unwrap_or_default();
            let terminus = trip.map(|x| x.terminus.clone()).unwrap_or_default();
            (
                t.number.clone(),
                t.trips.len(),
                t.days.clone(),
                t.runs,
                t.next_run.clone(),
                trip.map(|x| (x.departure, x.arrival)),
                from,
                terminus,
            )
        })
        .collect();
    let chosen_t = l.state.choice.tour.clone();
    let mut pick = None;
    l.ui.scroll_area(
        "tour-list",
        Rect::new(right.x - 4.0, right.y + 30.0, right.w + 8.0, right.h - 30.0),
        &mut |ui, v| {
            let rh = 84.0;
            for (k, (num, trips, days, runs, next, trip_time, from, terminus)) in
                tours.iter().enumerate()
            {
                let rr = Rect::new(v.x + 4.0, v.y + k as f32 * rh, v.w - 12.0, rh - 4.0);
                let on = chosen_t.as_deref() == Some(num.as_str());
                if ui.row(&format!("tour-{num}"), rr, on) {
                    pick = Some((num.clone(), *runs, next.clone()));
                }
                let c = if *runs { TEXT } else { TEXT_FAINT };
                // (the tour's name as the map writes it and OMSI lists it: "1", "Mo-Fr 1")
                let name = num.clone();
                ui.text_in(
                    &name,
                    Rect::new(rr.x + 10.0, rr.y + 5.0, rr.w - 110.0, 18.0),
                    13.5,
                    Weight::Bold,
                    c,
                    Align::Left,
                );
                if let Some((departure, arrival)) = trip_time {
                    ui.text_in(
                        &format!("{} - {}", hhmm(*departure), hhmm(*arrival)),
                        Rect::new(rr.right() - 110.0, rr.y + 6.0, 100.0, 18.0),
                        12.0,
                        Weight::Medium,
                        if *runs { ACCENT } else { TEXT_FAINT },
                        Align::Right,
                    );
                    ui.text_in(
                        &format!(
                            "{}: {}",
                            omsi_ui::tr("Trip duration"),
                            trip_duration(*departure, *arrival)
                        ),
                        Rect::new(rr.x + 10.0, rr.y + 43.0, rr.w - 20.0, 16.0),
                        12.0,
                        Weight::Medium,
                        c,
                        Align::Left,
                    );
                }
                if !from.is_empty() || !terminus.is_empty() {
                    ui.text_in(
                        &format!("{from} → {terminus}"),
                        Rect::new(rr.x + 10.0, rr.y + 25.0, rr.w - 20.0, 16.0),
                        11.5,
                        Weight::Medium,
                        c,
                        Align::Left,
                    );
                }
                let sub = if *runs {
                    format!("{trips} trips · {days}")
                } else {
                    match next {
                        Some(n) => format!("{trips} trips · {days} · runs {n}"),
                        None => format!("{trips} trips · never within a year"),
                    }
                };
                ui.text_in(
                    &sub,
                    Rect::new(rr.x + 10.0, rr.y + 61.0, rr.w - 20.0, 16.0),
                    11.0,
                    Weight::Regular,
                    TEXT_DIM,
                    Align::Left,
                );
            }
            tours.len() as f32 * rh + 4.0
        },
    );
    if let Some((num, runs, next)) = pick {
        // a tour of another day moves the date to the next day it runs (OMSI lists only
        // the day's tours)
        if !runs {
            if let Some(n) = next {
                l.state.choice.date = n;
                l.state.load_lines();
            }
        }
        l.state.choice.tour = Some(num);
        l.state.touched();
    }
}

pub(super) fn natural(s: &str) -> (u64, String) {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    (digits.parse().unwrap_or(u64::MAX), s.to_string())
}

/// Time from the departure to the arrival of one trip.
fn trip_duration(first: f64, last: f64) -> String {
    let minutes = ((last - first).max(0.0) / 60.0).round() as i64;
    let hours = minutes / 60;
    let remaining = minutes % 60;
    let count = |n: i64, one: &str, many: &str| {
        format!("{n} {}", omsi_ui::tr(if n == 1 { one } else { many }))
    };
    match (hours, remaining) {
        (0, m) => count(m, "minute", "minutes"),
        (h, 0) => count(h, "hour", "hours"),
        (h, m) => format!(
            "{} {}",
            count(h, "hour", "hours"),
            count(m, "minute", "minutes")
        ),
    }
}

/// The name of the server the Drive page is joined to (see the Multiplayer page).
fn joined_server_name(l: &Launcher) -> Option<String> {
    let a = l.state.joined_server.as_ref()?;
    let entry = l.state.servers.iter().find(|s| &s.address == a);
    let info = l.state.server_info.get(a).and_then(|x| x.1.as_ref().ok());
    Some(
        entry
            .map(|e| e.name.clone())
            .filter(|n| !n.is_empty())
            .or_else(|| info.map(|i| i.name.clone()))
            .unwrap_or_else(|| a.clone()),
    )
}

fn step_time(l: &mut Launcher, r: Rect) {
    let mut y = r.y;
    if let Some(name) = joined_server_name(l) {
        // the server's world: nothing to choose here
        let info = l
            .state
            .joined_server
            .as_ref()
            .and_then(|a| l.state.server_info.get(a))
            .and_then(|x| x.1.as_ref().ok())
            .cloned();
        l.ui.heading(
            Rect::new(r.x, y, r.w, 28.0),
            &format!("Set by {name}"),
            Some("lock"),
        );
        y += 36.0;
        let rows = [
            (
                "Time",
                info.as_ref().map(|i| i.time.clone()).unwrap_or_default(),
            ),
            (
                "Weather",
                info.as_ref()
                    .map(|i| {
                        if i.weather.is_empty() {
                            "the map's".to_string()
                        } else {
                            i.weather.clone()
                        }
                    })
                    .unwrap_or_default(),
            ),
        ];
        for (k, v) in rows {
            l.ui.text_in(
                k,
                Rect::new(r.x, y, 110.0, 22.0),
                13.0,
                Weight::Regular,
                TEXT_DIM,
                Align::Left,
            );
            l.ui.text_in(
                &v,
                Rect::new(r.x + 110.0, y, r.w - 110.0, 22.0),
                13.0,
                Weight::Regular,
                TEXT,
                Align::Left,
            );
            y += 26.0;
        }
        l.ui.paragraph("On a server the map, the time, the date and the weather are the same for everybody: the server keeps the world's clock. You choose your bus and your duty.", Vec2::new(r.x, y + 8.0), r.w, 12.5, Weight::Regular, TEXT_DIM);
        return;
    }
    let col = (r.w - 12.0) * 0.5;
    l.ui.label(Rect::new(r.x, y, col, 20.0), "Time");
    l.ui.label(Rect::new(r.x + col + 12.0, y, col, 20.0), "Date");
    y += 22.0;
    let mut t = l.state.choice.time;
    if l.ui
        .time_field("time", Rect::new(r.x, y, col, 44.0), &mut t)
    {
        l.state.choice.time = t;
        l.state.touched();
    }
    let mut d = l.state.choice.date.clone();
    if l.ui
        .date_field("date", Rect::new(r.x + col + 12.0, y, col, 44.0), &mut d)
    {
        l.state.choice.date = d;
        l.state.choice.season = "auto".into();
        l.state.load_lines();
        l.state.touched();
    }
    y += 54.0;
    let seasons = ["auto", "spring", "summer", "autumn", "winter"];
    let mut s = seasons
        .iter()
        .position(|x| *x == l.state.choice.season)
        .unwrap_or(0);
    if l.ui.segmented(
        "season",
        Rect::new(r.x, y, r.w, 34.0),
        &mut s,
        &["By date", "Spring", "Summer", "Autumn", "Winter"],
    ) {
        l.state.choice.season = seasons[s].to_string();
        if s > 0 {
            let month = ["", "04", "07", "10", "01"][s];
            let date = l.state.choice.date.clone();
            let (yy, dd) = (
                date.get(0..4).unwrap_or("1989").to_string(),
                date.get(8..10).unwrap_or("15").to_string(),
            );
            l.state.choice.date = format!("{yy}-{month}-{dd}");
            l.state.load_lines();
        }
        let w = l.state.choice.weather.clone();
        if let Some(wi) = l.state.weathers.iter().find(|x| x.file == w).cloned() {
            if !l.state.weather_fits(&wi) {
                l.state.choice.weather.clear();
            }
        }
        l.state.touched();
    }
    y += 46.0;
    let mut traffic = l.state.choice.traffic;
    if l.ui.slider(
        "traffic",
        Rect::new(r.x, y, r.w, 34.0),
        &mut traffic,
        0.0,
        120.0,
        1.0,
        "Cars around",
        &|v| format!("{v:.0}"),
    ) {
        l.state.choice.traffic = traffic;
        l.state.touched();
    }
    y += 40.0;
    let mut v = l.state.choice.passengers;
    if l.ui
        .toggle("pax", Rect::new(r.x, y, col, 32.0), &mut v, "Passengers")
    {
        l.state.choice.passengers = v;
        l.state.touched();
    }
    let mut v = l.state.choice.schedule;
    if l.ui.toggle(
        "sched",
        Rect::new(r.x + col + 12.0, y, col, 32.0),
        &mut v,
        "Timetable buses",
    ) {
        l.state.choice.schedule = v;
        l.state.touched();
    }
    y += 36.0;
    let mut v = l.state.choice.autostart;
    if l.ui.toggle(
        "autostart",
        Rect::new(r.x, y, r.w, 32.0),
        &mut v,
        "Put the bus into service on start (Shift+U)",
    ) {
        l.state.choice.autostart = v;
        l.state.touched();
    }
    y += 36.0;
    let mut v = l.state.choice.on_foot;
    if l.ui.toggle(
        "onfoot",
        Rect::new(r.x, y, r.w, 32.0),
        &mut v,
        "Start on foot (place a bus from the game menu)",
    ) {
        l.state.choice.on_foot = v;
        l.state.touched();
    }
    y += 44.0;
    // weather cards
    l.ui.heading(
        Rect::new(r.x, y, r.w, 28.0),
        "Weather",
        Some("partly_cloudy_day"),
    );
    y += 30.0;
    if selected_weather_as_custom(&l.state.config.root, &l.state.choice.weather).is_some() {
        if l.ui.button(
            "weather-edit-current",
            Rect::new(r.x, y, r.w, 34.0),
            "Edit selected weather as custom",
            Some("tune"),
            ButtonKind::Normal,
        ) {
            if let Some(custom) =
                selected_weather_as_custom(&l.state.config.root, &l.state.choice.weather)
            {
                l.state.choice.weather = custom;
                l.state.touched();
            }
        }
        y += 42.0;
    }
    let custom_now = crate::weather_setup::custom_weather(Some(&l.state.choice.weather));
    let custom_file = custom_now.clone().unwrap_or_default().encode();
    let custom_meta = custom_now
        .as_ref()
        .map(custom_weather_summary)
        .unwrap_or_else(|| "Set visibility, wind, clouds, rain, temperature and road state".into());
    let mut items: Vec<(String, String, String, String, bool)> = vec![
        (
            String::new(),
            "Map default".into(),
            "Whatever the map starts with".into(),
            "wb_sunny".into(),
            false,
        ),
        (
            custom_file,
            "Custom weather".into(),
            custom_meta,
            "tune".into(),
            false,
        ),
    ];
    // OMSI 2's current weather: an airport's METAR report, fetched when the game starts
    let metar = l
        .state
        .choice
        .weather
        .strip_prefix("metar:")
        .map(str::to_string);
    // (the airport nearest the map, not Berlin's for every map: Novi Sad got Berlin's rain)
    let home = nearest_airport(&l.state.config.root, &l.state.choice.map);
    items.push((
        format!("metar:{}", metar.clone().unwrap_or_else(|| home.clone())),
        "Current weather".into(),
        format!(
            "METAR of {} (fetched at the start)",
            metar.clone().unwrap_or_else(|| home.clone())
        ),
        "public".into(),
        false,
    ));
    // the weather going on from one to another through the day
    items.push((
        "cycle".into(),
        "Weather cycle".into(),
        "Changes every 25-60 minutes, as the month allows".into(),
        "autorenew".into(),
        false,
    ));
    for w in l.state.weathers.clone() {
        if !l.state.weather_fits(&w) {
            continue;
        }
        let vis = if w.fog_m >= 20000.0 {
            "clear air".to_string()
        } else {
            format!("{:.0} m", w.fog_m)
        };
        let icon = if w.snow || w.precip.starts_with("snow") {
            "weather_snowy"
        } else if w.precip.starts_with("rain") {
            "rainy"
        } else if w.fog_m < 1500.0 {
            "foggy"
        } else if w.clouds.to_lowercase().contains("overcast") {
            "cloud"
        } else if w.clouds.to_lowercase().contains("cumulus") {
            "partly_cloudy_day"
        } else {
            "wb_sunny"
        };
        items.push((
            w.file.clone(),
            w.name.clone(),
            format!("{:.0} °C · {} · {vis}", w.temp, w.precip),
            icon.into(),
            l.state.fresh.contains_key(&w.file),
        ));
    }
    if let Some(code) = metar.as_ref() {
        let root = std::path::PathBuf::from(&l.state.config.root);
        let list = crate::weather_setup::metar_airports(&root);

        let mut airport = code.to_uppercase().chars().take(4).collect::<String>();

        l.ui.label(Rect::new(r.x, y, 110.0, ROW), "Airport");

        let button_w = 150.0;
        let gap = 8.0;
        let input_w = r.w - 110.0 - button_w - gap;

        if l.ui.text_input(
            "metar-airport",
            Rect::new(r.x + 110.0, y, input_w, ROW),
            &mut airport,
            "ICAO",
            None,
        ) {
            airport = airport
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .take(4)
                .collect::<String>()
                .to_uppercase();

            l.state.choice.weather = if airport.is_empty() {
                "metar:".into()
            } else {
                format!("metar:{airport}")
            };
            l.state.touched();
        }
        let labels: Vec<String> = list.iter().map(|a| a.1.clone()).collect();
        let mut sel = list
            .iter()
            .position(|a| a.0.eq_ignore_ascii_case(&airport))
            .unwrap_or(0);

        if l.ui.select(
            "metar-airport-list",
            Rect::new(r.x + 110.0 + input_w + gap, y, button_w, ROW),
            &mut sel,
            &labels,
        ) {
            if let Some(a) = list.get(sel) {
                l.state.choice.weather = format!("metar:{}", a.0);
                l.state.touched();
            }
        }

        y += ROW + 8.0;
    }
    // When Custom weather is selected, the weather cards turn into the editor. The value is
    // still just `choice.weather`, so it survives launcher restarts and goes to the game
    // through the normal `--weather` argument.
    if let Some(mut custom) = crate::weather_setup::custom_weather(Some(&l.state.choice.weather)) {
        let area = Rect::new(r.x - 4.0, y, r.w + 8.0, r.bottom() - y);
        let mut changed = false;
        let mut presets = false;
        l.ui.scroll_area("custom-weather-editor", area, &mut |ui, v| {
            let mut yy = v.y + 4.0;
            if ui.button(
                "weather-presets",
                Rect::new(v.x + 4.0, yy, v.w - 12.0, 34.0),
                "Choose a weather preset",
                Some("arrow_back"),
                ButtonKind::Normal,
            ) {
                presets = true;
            }
            yy += 44.0;

            changed |= ui.slider(
                "custom-vis",
                Rect::new(v.x + 4.0, yy, v.w - 12.0, 34.0),
                &mut custom.visibility_m,
                50.0,
                50_000.0,
                50.0,
                "Visibility",
                &|x| {
                    if x >= 49_950.0 {
                        "unlimited".into()
                    } else if x >= 1000.0 {
                        format!("{:.1} km", x / 1000.0)
                    } else {
                        format!("{x:.0} m")
                    }
                },
            );
            yy += 40.0;
            changed |= ui.slider(
                "custom-bright",
                Rect::new(v.x + 4.0, yy, v.w - 12.0, 34.0),
                &mut custom.brightness,
                0.0,
                1.5,
                0.05,
                "Brightness",
                &|x| format!("{:.0} %", x * 100.0),
            );
            yy += 40.0;
            changed |= ui.slider(
                "custom-wdir",
                Rect::new(v.x + 4.0, yy, v.w - 12.0, 34.0),
                &mut custom.wind_dir,
                0.0,
                355.0,
                5.0,
                "Wind direction",
                &|x| format!("{x:.0}°"),
            );
            yy += 40.0;
            changed |= ui.slider(
                "custom-wspeed",
                Rect::new(v.x + 4.0, yy, v.w - 12.0, 34.0),
                &mut custom.wind_speed,
                0.0,
                40.0,
                0.5,
                "Wind speed",
                &|x| format!("{x:.1} m/s"),
            );
            yy += 40.0;
            changed |= ui.slider(
                "custom-temp",
                Rect::new(v.x + 4.0, yy, v.w - 12.0, 34.0),
                &mut custom.temp_c,
                -30.0,
                45.0,
                1.0,
                "Temperature",
                &|x| format!("{x:.0} °C"),
            );
            yy += 40.0;
            let temp_for_dew = custom.temp_c;
            changed |= ui.slider(
                "custom-hum",
                Rect::new(v.x + 4.0, yy, v.w - 12.0, 34.0),
                &mut custom.humidity,
                0.0,
                100.0,
                1.0,
                "Humidity",
                &|x| {
                    format!(
                        "{x:.0} % · dew {:.0} °C",
                        crate::weather_setup::dew_point_c(temp_for_dew, x)
                    )
                },
            );
            yy += 44.0;

            ui.label(Rect::new(v.x + 4.0, yy, 130.0, 32.0), "Cloud type");
            let cloud_labels: Vec<String> = crate::weather_setup::CUSTOM_CLOUDS
                .iter()
                .map(|x| (*x).to_string())
                .collect();
            let mut cloud = custom.cloud;
            if ui.select(
                "custom-cloud",
                Rect::new(v.x + 134.0, yy, v.w - 142.0, 32.0),
                &mut cloud,
                &cloud_labels,
            ) {
                custom.cloud = cloud;
                changed = true;
            }
            yy += 44.0;

            ui.label(Rect::new(v.x + 4.0, yy, 130.0, 32.0), "Precipitation");
            let precip_labels: Vec<String> = crate::weather_setup::CUSTOM_PRECIP
                .iter()
                .map(|x| (*x).to_string())
                .collect();
            let mut precip = custom.precip.clamp(0, 2) as usize;
            if ui.select(
                "custom-precip",
                Rect::new(v.x + 134.0, yy, v.w - 142.0, 32.0),
                &mut precip,
                &precip_labels,
            ) {
                custom.precip = precip as i32;
                changed = true;
            }
            yy += 40.0;
            changed |= ui.slider(
                "custom-intensity",
                Rect::new(v.x + 4.0, yy, v.w - 12.0, 34.0),
                &mut custom.precip_intensity,
                0.0,
                255.0,
                1.0,
                "Precipitation intensity",
                &|x| format!("{x:.0} / 255"),
            );
            yy += 40.0;
            changed |= ui.slider(
                "custom-wet",
                Rect::new(v.x + 4.0, yy, v.w - 12.0, 34.0),
                &mut custom.road_wetness,
                0.0,
                1.0,
                0.05,
                "Road wetness",
                &|x| format!("{:.0} %", x * 100.0),
            );
            yy += 40.0;

            let mut snow = custom.snow_cover;
            if ui.toggle(
                "custom-snow",
                Rect::new(v.x + 4.0, yy, (v.w - 20.0) * 0.5, 32.0),
                &mut snow,
                "Snow cover",
            ) {
                custom.snow_cover = snow;
                changed = true;
            }
            let mut snow_road = custom.snow_on_road;
            if ui.toggle(
                "custom-snow-road",
                Rect::new(
                    v.x + 12.0 + (v.w - 20.0) * 0.5,
                    yy,
                    (v.w - 20.0) * 0.5,
                    32.0,
                ),
                &mut snow_road,
                "Snow on road",
            ) {
                custom.snow_on_road = snow_road;
                changed = true;
            }
            yy += 44.0;
            yy - v.y
        });
        if presets {
            l.state.choice.weather.clear();
            l.state.touched();
        } else if changed {
            custom.normalize();
            l.state.choice.weather = custom.encode();
            l.state.touched();
        }
        return;
    }

    let chosen = l.state.choice.weather.clone();
    let mut pick = None;
    let area = Rect::new(r.x - 4.0, y, r.w + 8.0, r.bottom() - y);
    l.ui.scroll_area("weather", area, &mut |ui, v| {
        let cw = (v.w - 20.0) / 2.0;
        let ch = 62.0;
        for (k, (file, name, meta, icon, fresh)) in items.iter().enumerate() {
            let (cx, cy) = ((k % 2) as f32, (k / 2) as f32);
            let rr = Rect::new(v.x + 4.0 + cx * (cw + 8.0), v.y + cy * (ch + 8.0), cw, ch);
            let on = *file == chosen;
            let id = id_of(&format!("w-{file}"));
            let (h, _, clicked) = ui.interact(id, rr);
            if clicked {
                pick = Some(file.clone());
            }
            let t = ui.anim(id, if h { 1.0 } else { 0.0 }, 0.08);
            ui.p()
                .rounded(rr, 6.0, if on { SELECTED } else { FIELD.mix(HOVER, t) });
            ui.p()
                .rounded_border(rr, 6.0, 1.0, if on { ACCENT.alpha(0.6) } else { EDGE });
            ui.icon(
                icon,
                Vec2::new(rr.x + 24.0, rr.center().y),
                22.0,
                if on { TEXT } else { TEXT_DIM },
            );
            let tw = ui.text_in(
                name,
                Rect::new(rr.x + 46.0, rr.y + 10.0, rr.w - 56.0, 20.0),
                13.0,
                Weight::Bold,
                TEXT,
                Align::Left,
            );
            if *fresh {
                ui.badge(Vec2::new(rr.x + 50.0 + tw, rr.y + 12.0), "NEW", OK);
            }
            ui.text_in(
                meta,
                Rect::new(rr.x + 46.0, rr.y + 33.0, rr.w - 56.0, 18.0),
                11.0,
                Weight::Regular,
                TEXT_DIM,
                Align::Left,
            );
        }
        ((items.len() + 1) / 2) as f32 * (ch + 8.0) + 4.0
    });
    if let Some(f) = pick {
        l.state.choice.weather = f;
        l.state.touched();
    }
}

fn step_roadbook(l: &mut Launcher, r: Rect) {
    let (Some(line), Some(tour), false) = (
        l.state.line().cloned(),
        l.state.tour().cloned(),
        l.state.choice.free,
    ) else {
        let map = l
            .state
            .map()
            .map(|m| {
                if m.friendly.is_empty() {
                    m.name.clone()
                } else {
                    m.friendly.clone()
                }
            })
            .unwrap_or_default();
        l.ui.paragraph(&format!("No duty chosen: free driving on {map}. Pick a line and a tour under Route to see the roadbook here."), Vec2::new(r.x, r.y), r.w, 13.0, Weight::Regular, TEXT_DIM);
        ibis_box(l, Rect::new(r.x, r.y + 60.0, r.w, 160.0));
        return;
    };
    let from = l.state.first_trip().unwrap_or(0);
    l.ui.text_in(
        &format!(
            "Line {} · tour {} · from {}",
            line.name,
            tour.number,
            hhmm(l.state.choice.time as f64 * 60.0)
        ),
        Rect::new(r.x, r.y, r.w, 22.0),
        14.0,
        Weight::Bold,
        TEXT,
        Align::Left,
    );
    l.ui.text_in(
        "Click a trip to start the tour there.",
        Rect::new(r.x, r.y + 20.0, r.w, 16.0),
        11.5,
        Weight::Regular,
        TEXT_DIM,
        Align::Left,
    );
    let trips: Vec<omsi_launcher_lib::TripInfo> = tour.trips.clone();
    let ibis_h = 150.0;
    let list = Rect::new(r.x - 4.0, r.y + 40.0, r.w + 8.0, r.h - 40.0 - ibis_h - 10.0);
    let mut start_at = None;
    l.ui.scroll_area("roadbook", list, &mut |ui, v| {
        let mut y = v.y;
        for (i, t) in trips.iter().enumerate() {
            let head = Rect::new(v.x + 4.0, y, v.w - 12.0, 46.0);
            if i < from {
                if ui.row(&format!("roadbook-trip-{i}"), head, false) {
                    start_at = Some((t.index, t.departure));
                }
                ui.text_in(
                    &format!(
                        "{} · {} → {}",
                        omsi_ui::tr("Earlier"),
                        if t.from.is_empty() { "?" } else { &t.from },
                        t.terminus
                    ),
                    Rect::new(head.x + 10.0, head.y + 4.0, head.w - 20.0, 20.0),
                    13.0,
                    Weight::Bold,
                    TEXT_FAINT,
                    Align::Left,
                );
                ui.text_in(
                    &format!(
                        "{} - {} · {}",
                        hhmm(t.departure),
                        hhmm(t.arrival),
                        if t.line.is_empty() {
                            "depot run".to_string()
                        } else {
                            format!("line {}", t.line)
                        }
                    ),
                    Rect::new(head.x + 10.0, head.y + 24.0, head.w - 20.0, 18.0),
                    11.0,
                    Weight::Regular,
                    TEXT_FAINT,
                    Align::Left,
                );
                y += 52.0;
                continue;
            }
            let k = i - from;
            if k == 0 {
                ui.p().rounded(head, 6.0, SELECTED);
            } else if ui.row(&format!("roadbook-trip-{i}"), head, false) {
                start_at = Some((t.index, t.departure));
            }
            ui.text_in(
                &format!(
                    "{} · {} → {}",
                    if k == 0 { "Your first trip" } else { "Then" },
                    if t.from.is_empty() { "?" } else { &t.from },
                    t.terminus
                ),
                Rect::new(head.x + 10.0, head.y + 4.0, head.w - 20.0, 20.0),
                13.0,
                Weight::Bold,
                TEXT,
                Align::Left,
            );
            ui.text_in(
                &format!(
                    "{} - {} · {:.1} km · {}{}",
                    hhmm(t.departure),
                    hhmm(t.arrival),
                    t.km,
                    if t.line.is_empty() {
                        "depot run".to_string()
                    } else {
                        format!("line {}", t.line)
                    },
                    format!(" · {}", t.name)
                ),
                Rect::new(head.x + 10.0, head.y + 24.0, head.w - 20.0, 18.0),
                11.0,
                Weight::Regular,
                TEXT_DIM,
                Align::Left,
            );
            y += 52.0;
            let n = t.stops.len();
            for (s, st) in t.stops.iter().enumerate() {
                let rr = Rect::new(v.x + 4.0, y, v.w - 12.0, 24.0);
                // the timeline: a line with a dot per stop
                let cx = rr.x + 60.0;
                if s + 1 < n {
                    ui.p().rect(
                        Rect::new(cx - 1.0, rr.center().y, 2.0, 24.0),
                        Color::WHITE.alpha(0.12),
                    );
                }
                let end = s == 0 || s + 1 == n;
                ui.p().circle(
                    Vec2::new(cx, rr.center().y),
                    if end { 4.0 } else { 3.0 },
                    if end { TEXT } else { TEXT_FAINT },
                );
                ui.text_in(
                    &hhmm(st.arr),
                    Rect::new(rr.x + 6.0, rr.y, 42.0, rr.h),
                    12.0,
                    Weight::Condensed,
                    if end { TEXT } else { TEXT_SOFT },
                    Align::Left,
                );
                ui.text_in(
                    &st.name,
                    Rect::new(cx + 14.0, rr.y, rr.w - 140.0, rr.h),
                    12.5,
                    if end { Weight::Bold } else { Weight::Regular },
                    TEXT,
                    Align::Left,
                );
                if s == 0 {
                    ui.text_in(
                        &format!("dep {}", hhmm(st.dep)),
                        Rect::new(rr.right() - 80.0, rr.y, 76.0, rr.h),
                        11.0,
                        Weight::Medium,
                        TEXT_DIM,
                        Align::Right,
                    );
                }
                y += 24.0;
            }
            y += 12.0;
        }
        y - v.y
    });
    if let Some((index, dep)) = start_at {
        let time = (dep / 60.0).floor() as i32;
        l.state.choice.time = time;
        l.state.choice.start_trip = Some((line.name.clone(), tour.number.clone(), index, time));
        l.state.touched();
    }
    ibis_box(l, Rect::new(r.x, r.bottom() - ibis_h, r.w, ibis_h));
}

pub(super) fn ibis_box(l: &mut Launcher, r: Rect) {
    l.ui.p().rounded(r, 6.0, FIELD);
    let inner = l.ui.heading(
        Rect::new(r.x + 12.0, r.y + 10.0, r.w - 24.0, r.h - 20.0),
        "IBIS",
        Some("keyboard"),
    );
    let Some((_, info)) = l.state.ibis.clone() else {
        l.ui.paragraph("Pick a line to see what to type into the IBIS. Shift+U in the game types it for you and puts the bus into service.", Vec2::new(inner.x, inner.y - 4.0), inner.w, 12.0, Weight::Regular, TEXT_DIM);
        return;
    };
    match info {
        Err(e) => {
            l.ui.paragraph(
                &e,
                Vec2::new(inner.x, inner.y - 4.0),
                inner.w,
                12.0,
                Weight::Regular,
                DANGER,
            );
        }
        Ok(i) if i.routes.is_empty() => {
            l.ui.paragraph(&format!("Depot file {} has no entries for this line. Type line {} and the terminus code by hand, or press Shift+U.", i.hof, i.line_code), Vec2::new(inner.x, inner.y - 4.0), inner.w, 12.0, Weight::Regular, TEXT_DIM);
        }
        Ok(i) => {
            let mut y = inner.y - 2.0;
            for rt in i.routes.iter().take(3) {
                let code = rt.code.clone();
                let (lc, rc) = if code.len() > 2 {
                    (
                        code[..code.len() - 2].to_string(),
                        code[code.len() - 2..].to_string(),
                    )
                } else {
                    (i.line_code.clone(), code.clone())
                };
                let name = if rt.name.is_empty() {
                    rt.terminus.clone()
                } else {
                    rt.name.clone()
                };
                l.ui.text_in(
                    &name,
                    Rect::new(inner.x, y, inner.w - 170.0, 22.0),
                    12.5,
                    Weight::Medium,
                    TEXT,
                    Align::Left,
                );
                let mut x = inner.right() - 160.0;
                for (label, v) in [("line", lc), ("route", rc)] {
                    l.ui.text_in(
                        label,
                        Rect::new(x, y, 34.0, 22.0),
                        11.0,
                        Weight::Regular,
                        TEXT_DIM,
                        Align::Left,
                    );
                    let cw = l.ui.width(&v, 13.0, Weight::Black) + 12.0;
                    let cr = Rect::new(x + 34.0, y + 2.0, cw, 18.0);
                    l.ui.p().rounded(cr, 4.0, SELECTED);
                    l.ui.text_in(&v, cr, 12.5, Weight::Bold, TEXT, Align::Center);
                    x += 80.0;
                }
                y += 26.0;
            }
            l.ui.text_in(
                &format!(
                    "Depot file {} · Shift+U in the game types it for you",
                    i.hof
                ),
                Rect::new(inner.x, y + 2.0, inner.w, 18.0),
                11.0,
                Weight::Regular,
                TEXT_FAINT,
                Align::Left,
            );
        }
    }
}

/// The right column: the bus preview, the duty in short, the start button.
fn summary(l: &mut Launcher, side: Rect) {
    let pw = side.w;
    let ph = (pw / 1.6).min(side.h * 0.55);
    let pr = Rect::new(side.x, side.y, pw, ph);
    l.preview(pr);
    let mut y = pr.bottom() + 18.0;
    let bus_name = l
        .state
        .bus()
        .map(|bus| display_bus_name(&bus.name))
        .unwrap_or_else(|| "No bus chosen".into());
    l.ui.text_in(
        &bus_name,
        Rect::new(side.x, y, pw, 24.0),
        18.0,
        Weight::Bold,
        TEXT,
        Align::Left,
    );
    y += 26.0;
    let paint = if l.state.choice.paint.is_empty() {
        l.state
            .bus()
            .map(default_livery_label)
            .unwrap_or("Default paint")
            .to_string()
    } else {
        l.state.choice.paint.clone()
    };
    l.ui.text_in(
        &paint,
        Rect::new(side.x, y, pw, 18.0),
        12.5,
        Weight::Regular,
        TEXT_DIM,
        Align::Left,
    );
    y += 30.0;
    let map = l
        .state
        .map()
        .map(|m| {
            if m.friendly.is_empty() {
                m.name.clone()
            } else {
                m.friendly.clone()
            }
        })
        .unwrap_or_else(|| "-".into());
    let duty = match (
        &l.state.choice.line,
        &l.state.choice.tour,
        l.state.choice.free,
    ) {
        (_, _, true) | (None, _, _) => "Free drive".to_string(),
        (Some(line), Some(t), _) => format!("Line {line}, tour {t}"),
        (Some(line), None, _) => format!("Line {line}, choose a tour"),
    };
    let weather = match l.state.choice.weather.strip_prefix("metar:") {
        Some(code) => format!("Current weather at {code}"),
        None if l.state.choice.weather == "cycle" => "Weather cycle".into(),
        None if crate::weather_setup::custom_weather(Some(&l.state.choice.weather)).is_some() => {
            let c = crate::weather_setup::custom_weather(Some(&l.state.choice.weather)).unwrap();
            format!("Custom · {}", custom_weather_summary(&c))
        }
        None => l
            .state
            .weathers
            .iter()
            .find(|w| w.file == l.state.choice.weather)
            .map(|w| w.name.clone())
            .unwrap_or_else(|| "Map default".into()),
    };
    let (yy, mm, dd) = super::ui::parse_date(&l.state.choice.date);
    let mut start_at = format!(
        "{:02}:{:02}, {dd} {} {yy}",
        l.state.choice.time / 60,
        l.state.choice.time % 60,
        super::ui::MONTHS[(mm as usize).clamp(1, 12) - 1]
    );
    let mut weather = weather;
    // on a server: its clock and weather
    if let Some(i) = l
        .state
        .joined_server
        .as_ref()
        .and_then(|a| l.state.server_info.get(a))
        .and_then(|x| x.1.as_ref().ok())
    {
        start_at = format!("{} (the server's clock)", i.time);
        weather = if i.weather.is_empty() {
            "the map's (the server's)".into()
        } else {
            format!("{} (the server's)", i.weather)
        };
    }
    let rows = [
        ("Map", map),
        ("Duty", duty),
        ("Start", start_at),
        ("Weather", weather),
    ];
    for (k, v) in rows {
        l.ui.text_in(
            k,
            Rect::new(side.x, y, 90.0, 22.0),
            12.5,
            Weight::Regular,
            TEXT_DIM,
            Align::Left,
        );
        l.ui.text_in(
            &v,
            Rect::new(side.x + 90.0, y, pw - 90.0, 22.0),
            12.5,
            Weight::Regular,
            TEXT,
            Align::Left,
        );
        y += 24.0;
    }
    if let Some(b) = l.state.bus().filter(|b| !b.missing_packs.is_empty()) {
        y += 8.0;
        let text = omsi_ui::tr("Parts missing: needs %{packs}")
            .replace("%{packs}", &b.missing_packs.join(", "));
        l.ui.icon("warning", Vec2::new(side.x + 9.0, y + 10.0), 16.0, WARN);
        l.ui.text_in(
            &text,
            Rect::new(side.x + 24.0, y, pw - 24.0, 20.0),
            12.5,
            Weight::Medium,
            WARN,
            Align::Left,
        );
    }
    // on a server: which, and the way back to playing alone
    if let Some(name) = joined_server_name(l) {
        y += 12.0;
        l.ui.icon("dns", Vec2::new(side.x + 9.0, y + 11.0), 16.0, OK);
        l.ui.text_in(
            &format!("Server: {name}"),
            Rect::new(side.x + 24.0, y, pw - 170.0, 22.0),
            13.0,
            Weight::Medium,
            OK,
            Align::Left,
        );
        if l.ui.button(
            "leave-server",
            Rect::new(side.x + pw - 150.0, y - 6.0, 150.0, 34.0),
            "Leave Server",
            Some("logout"),
            ButtonKind::Danger,
        ) {
            l.state.leave_server();
        }
    }
    let running = l.state.instances.iter().filter(|i| i.running).count();
    // the start: the whole width of the column, at its foot
    let note_h = if running > 0 { 24.0 } else { 0.0 };
    let btn = Rect::new(side.x, side.bottom() - 48.0 - note_h, pw, 48.0);
    let free = l.state.choice.free || l.state.choice.line.is_none();
    let label = if running > 0
        && l.state
            .second_armed
            .map(|t| t.elapsed().as_secs() < 6)
            .unwrap_or(false)
    {
        "Start another game"
    } else if free && l.state.joined_server.is_none() {
        "Drive"
    } else {
        "Start the duty"
    };
    if l.ui.button(
        "launch",
        btn,
        label,
        Some("play_arrow"),
        ButtonKind::Primary,
    ) {
        start(l);
    }
    // where the last game on this map was left (`laststn.osn`): a second way in
    // (with save slots of the map, #341: which one, beside the button)
    if l.state.joined_server.is_none() && l.state.has_last_situation() {
        let saves: Vec<String> = l
            .state
            .saved_situations()
            .iter()
            .map(|s| s.name.clone())
            .collect();
        let cont = Rect::new(btn.x, btn.y - 44.0, pw, 38.0);
        if saves.len() > 1 {
            let sw = (pw * 0.5).round();
            let mut pick = l.state.save_pick.min(saves.len() - 1);
            if l.ui.select(
                "continue-which",
                Rect::new(cont.x, cont.y, sw, cont.h),
                &mut pick,
                &saves,
            ) {
                l.state.save_pick = pick;
            }
            if l.ui.button(
                "continue",
                Rect::new(cont.x + sw + 8.0, cont.y, pw - sw - 8.0, cont.h),
                "Continue",
                Some("history"),
                ButtonKind::Normal,
            ) {
                l.state.launch_last_situation();
            }
        } else if l.ui.button(
            "continue",
            cont,
            "Continue where you left off",
            Some("history"),
            ButtonKind::Normal,
        ) {
            l.state.launch_last_situation();
        }
    }
    if running > 0 {
        let note = format!(
            "{running} game{} running - see Sessions",
            if running > 1 { "s" } else { "" }
        );
        let nr = Rect::new(btn.x, btn.bottom() + 4.0, pw, 20.0);
        let (h, _, clicked) = l.ui.interact(id_of("running-note"), nr);
        if clicked {
            l.go(super::Page::Sessions);
        }
        l.ui.text_in(
            &note,
            nr,
            12.5,
            Weight::Regular,
            if h { TEXT } else { OK },
            Align::Center,
        );
    }
    let _ = fmt_bytes;
}

/// The phone's Start: as the desktop's.
pub(super) fn start_from_phone(l: &mut Launcher) {
    start(l);
}

fn start(l: &mut Launcher) {
    if l.state.bus().is_none() || l.state.map().is_none() {
        l.state.set_status("Choose a bus and a map first.", true);
        return;
    }
    if l.state.choice.lan_mode == "join" && !l.state.join.0 {
        let t = l.state.join.1.clone();
        l.state.set_status(format!("LAN: {t}"), true);
        return;
    }
    let running = l.state.instances.iter().filter(|i| i.running).count();
    // a second game on one computer is for testing LAN play, not something to do by
    // accident: with one running, the button asks for a second click
    if running > 0
        && l.state
            .second_armed
            .map(|t| t.elapsed().as_secs() >= 6)
            .unwrap_or(true)
    {
        l.state.second_armed = Some(std::time::Instant::now());
        l.state.set_status("A game is running already (its window may be behind this one - see Sessions). Click again to start another one anyway.", true);
        return;
    }
    l.state.second_armed = None;
    l.state.choice.save();
    l.state.launch();
    if l.state.choice.lan_mode != "off" {
        l.go(super::Page::Sessions);
    }
}

/// The airport of OMSI's METAR list (`Weather/ICAO.txt`) nearest to where map `map` lies
/// (its `timezone.txt`), else Berlin's; read once per map.
pub(super) fn custom_weather_summary(c: &crate::weather_setup::CustomWeather) -> String {
    let precip = match c.precip {
        1 => format!("rain {:.0}%", c.precip_intensity / 255.0 * 100.0),
        2 => format!("snow {:.0}%", c.precip_intensity / 255.0 * 100.0),
        _ => "dry".to_string(),
    };
    let vis = if c.visibility_m >= 49_950.0 {
        "clear visibility".to_string()
    } else if c.visibility_m >= 1000.0 {
        format!("{:.1} km", c.visibility_m / 1000.0)
    } else {
        format!("{:.0} m", c.visibility_m)
    };
    format!(
        "{:.0} °C · {:.0}% RH · {precip} · {vis}",
        c.temp_c, c.humidity
    )
}

pub(super) fn selected_weather_as_custom(root: &str, file: &str) -> Option<String> {
    if file.is_empty()
        || file == "cycle"
        || file.to_ascii_lowercase().starts_with("metar:")
        || crate::weather_setup::custom_weather(Some(file)).is_some()
    {
        return None;
    }
    let path = omsi_cfg::resolve_path(std::path::Path::new(root), file);
    let w = omsi_content::weather::Weather::load(&path).ok()?;
    let wet = (w.ground_wet[0] / 255.0).clamp(0.0, 1.0);
    Some(crate::weather_setup::CustomWeather::from_weather(&w, 1.0, wet).encode())
}

pub(crate) fn nearest_airport(root: &str, map: &str) -> String {
    static CACHE: std::sync::Mutex<Option<hashbrown::HashMap<String, String>>> =
        std::sync::Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(Default::default);
    if let Some(c) = cache.get(map) {
        return c.clone();
    }
    // (the airports of the list round Europe and a little beyond, where OMSI's maps lie)
    const AIRPORTS: &[(&str, f64, f64)] = &[
        ("BKPR", 42.57, 21.04),
        ("EBBR", 50.90, 4.48),
        ("EDDH", 53.63, 9.99),
        ("EDDM", 48.35, 11.79),
        ("EDDN", 49.50, 11.08),
        ("EDDP", 51.42, 12.24),
        ("EDDS", 48.69, 9.22),
        ("EDDB", 52.37, 13.52),
        ("EDOP", 53.43, 11.78),
        ("EETN", 59.41, 24.83),
        ("EFHF", 60.25, 25.04),
        ("EGAA", 54.66, -6.22),
        ("EGCC", 53.35, -2.27),
        ("EGLL", 51.47, -0.45),
        ("EGPH", 55.95, -3.37),
        ("EHAM", 52.31, 4.76),
        ("EIDW", 53.42, -6.27),
        ("EKBI", 55.74, 9.15),
        ("ELLX", 49.63, 6.21),
        ("ENBR", 60.29, 5.22),
        ("ENGM", 60.19, 11.10),
        ("ESKN", 58.79, 16.91),
        ("EPKK", 50.08, 19.78),
        ("EPWA", 52.17, 20.97),
        ("EVRA", 56.92, 23.97),
        ("EYVI", 54.63, 25.29),
        ("LBSF", 42.70, 23.41),
        ("LDZA", 45.74, 16.07),
        ("LEAL", 38.28, -0.56),
        ("LEBB", 43.30, -2.91),
        ("LEMD", 40.47, -3.56),
        ("LEPA", 39.55, 2.74),
        ("LFBD", 44.83, -0.72),
        ("LFLY", 45.73, 5.08),
        ("LFMD", 43.54, 6.95),
        ("LFPG", 49.01, 2.55),
        ("LGAV", 37.94, 23.94),
        ("LHBP", 47.44, 19.26),
        ("LICJ", 38.18, 13.10),
        ("LIMC", 45.63, 8.72),
        ("LIRA", 41.80, 12.59),
        ("LOWI", 47.26, 11.34),
        ("LOWL", 48.23, 14.19),
        ("LOWW", 48.11, 16.57),
        ("LPPT", 38.78, -9.14),
        ("LQSA", 43.82, 18.33),
        ("LSGG", 46.24, 6.11),
        ("LSZH", 47.46, 8.55),
        ("LTAC", 40.13, 32.99),
        ("LWSK", 41.96, 21.62),
        ("LYBE", 44.82, 20.31),
        ("LYTV", 42.40, 18.72),
        ("LZIB", 48.17, 17.21),
        ("UKKK", 50.40, 30.45),
        ("ULLI", 59.80, 30.26),
        ("UMKK", 54.89, 20.59),
        ("UMMM", 53.88, 28.03),
        ("UUEE", 55.97, 37.41),
        ("USSS", 56.74, 60.80),
    ];
    let dir = std::path::Path::new(map)
        .parent()
        .map(|d| d.to_string_lossy().to_string())
        .unwrap_or_default();
    let tz = omsi_cfg::resolve_path(std::path::Path::new(root), &format!("{dir}/timezone.txt"));
    let code = omsi_map::TimeZone::load(&tz)
        .ok()
        .and_then(|t| t.lat_lon())
        .and_then(|(lat, lon)| {
            AIRPORTS
                .iter()
                .map(|(c, a, o)| {
                    (
                        c,
                        (a - lat).powi(2) + ((o - lon) * lat.to_radians().cos()).powi(2),
                    )
                })
                .min_by(|x, y| x.1.total_cmp(&y.1))
                .map(|x| x.0.to_string())
        })
        .unwrap_or_else(|| "EDDB".into());
    cache.insert(map.to_string(), code.clone());
    code
}

#[cfg(test)]
mod vehicle_picker_tests {
    use super::*;

    fn vehicle(
        folder: &str,
        maker: &str,
        name: &str,
        file: &str,
    ) -> omsi_launcher_lib::VehicleInfo {
        omsi_launcher_lib::VehicleInfo {
            name: format!("{maker} {name}"),
            manufacturer: maker.into(),
            type_name: name.into(),
            folder: folder.into(),
            file: format!("Vehicles/{folder}/{file}.bus"),
            description: String::new(),
            paints: vec!["Paint".into()],
            hofs: vec![],
            installed: false,
            missing_packs: vec![],
            numbers: vec![],
            default_paint: "Beige".into(),
        }
    }

    #[test]
    fn empty_vehicle_types_use_the_file_name() {
        for empty in ["", "   "] {
            let manufacturers = build_bus_manufacturers(
                &[vehicle("Pack", "MAN", empty, "NL_202")],
                None,
                &Default::default(),
            );
            assert_eq!(manufacturers[0].variants[0].variant, "NL 202");
        }
    }

    #[test]
    fn omsi_manufacturer_groups_dl_and_lions_city_across_packs() {
        let vehicles = vec![
            vehicle("MAN_DL05", "MAN", "DL05", "dl05"),
            vehicle("MAN_DL05", "MAN", "DL07", "dl07"),
            vehicle("MAN_DL05", "MAN", "DL08", "dl08"),
            vehicle("MAN_DL05", "MAN", "DL09", "dl09"),
            vehicle("MAN_LC_MVG", "MAN", "Lion's City (MVG)", "mvg"),
            vehicle("MAN_LC_GUE", "MAN", "Lion's City G (ORN)", "orn"),
        ];
        let manufacturers = build_bus_manufacturers(&vehicles, None, &Default::default());
        assert_eq!(manufacturers.len(), 1);
        assert_eq!(manufacturers[0].name, "MAN");
        assert_eq!(manufacturers[0].variants.len(), 6);
        assert!(
            manufacturers[0]
                .variants
                .iter()
                .any(|v| v.variant == "DL07")
        );
        assert!(
            manufacturers[0]
                .variants
                .iter()
                .any(|v| v.variant == "Lion's City G (ORN)")
        );
        assert!(manufacturer_matches(&manufacturers[0], "orn"));
        assert!(!manufacturer_matches(&manufacturers[0], "not a bus"));
    }

    #[test]
    fn author_defined_manufacturer_and_complete_type_are_preserved() {
        let vehicles = vec![
            vehicle(
                "Pack",
                "Mercedes-Benz Release",
                "MB_C2_E6_GN_BVG_Leasing 2",
                "leasing",
            ),
            vehicle("Pack", "Mercedes-Benz", "O530", "o530"),
        ];
        let manufacturers = build_bus_manufacturers(&vehicles, None, &Default::default());
        assert_eq!(manufacturers.len(), 2);
        let maker = manufacturers
            .iter()
            .find(|m| m.name == "Mercedes-Benz Release")
            .unwrap();
        assert_eq!(maker.variants[0].variant, "MB C2 E6 GN BVG Leasing 2");
    }

    #[test]
    fn duplicate_names_remain_selectable_and_host_filter_is_respected() {
        let vehicles = vec![
            vehicle("MAN", "MAN", "NL202", "en92"),
            vehicle("MAN", "MAN", "NL202", "en93"),
            vehicle("OtherPack", "MAN", "NL202", "en92"),
        ];
        let manufacturers = build_bus_manufacturers(&vehicles, None, &Default::default());
        let labels: std::collections::HashSet<_> = manufacturers[0]
            .variants
            .iter()
            .map(|v| &v.variant)
            .collect();
        assert_eq!(labels.len(), 3);
        let allowed = std::collections::HashSet::from([vehicles[1].file.to_lowercase()]);
        let filtered = build_bus_manufacturers(&vehicles, Some(&allowed), &Default::default());
        assert_eq!(filtered[0].variants.len(), 1);
        assert_eq!(filtered[0].variants[0].file, vehicles[1].file);
    }

    #[test]
    fn numbers_in_type_names_sort_naturally_and_default_livery_has_its_omsi_name() {
        assert_eq!(bus_name_cmp("DL9", "DL10"), std::cmp::Ordering::Less);
        assert_eq!(bus_name_cmp("MAN", "man"), std::cmp::Ordering::Equal);
        let mut vehicle = vehicle("MAN", "MAN", "DL07", "dl07");
        assert_eq!(default_livery_label(&vehicle), "Beige");
        vehicle.default_paint.clear();
        assert_eq!(default_livery_label(&vehicle), "Default paint");
    }
}
