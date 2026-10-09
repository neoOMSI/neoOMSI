//! Vehicle pages and menu.

use super::*;

pub(crate) fn vehicle_pages(app: &App) -> Vec<Page> {
    let has = app.player.is_some();
    let server = crate::input_script::on_server(&app.args);
    let mut display: Vec<(String, String)> = Vec::new();
    if has {
        display.push(opens(
            &tx("pause.page.vehicle.action.dest.name"),
            &tx("pause.page.vehicle.action.dest.desc"),
            "dest",
        ));
        display.push(opens(
            &tx("pause.page.vehicle.action.hof.name"),
            &tx("pause.page.vehicle.action.hof.desc"),
            "hof",
        ));
        display.push(opens(
            &tx("pause.page.vehicle.action.number.name"),
            &tx("pause.page.vehicle.action.number.desc"),
            "number",
        ));
    }
    let mut fleet: Vec<(String, String)> = Vec::new();
    if has || !app.placed.is_empty() {
        fleet.push(button(
            &tx("pause.page.vehicle.action.switch.name"),
            &tx("pause.page.vehicle.action.switch.button"),
            &tx("pause.page.vehicle.action.switch.desc"),
            "switch",
        ));
    }
    fleet.push(opens(
        &tx("pause.page.vehicle.action.place.name"),
        &tx("pause.page.vehicle.action.place.desc"),
        "place",
    ));
    if has {
        // (#728: another bus in this one's place, or this one again with its files read
        // anew - a script or a .bus changed - without starting the game again)
        fleet.push(button(
            &tx("pause.page.vehicle.action.swap.name"),
            &tx("pause.page.vehicle.action.swap.button"),
            &tx("pause.page.vehicle.action.swap.desc"),
            "swap",
        ));
        fleet.push(button(
            &tx("pause.page.vehicle.action.couple.name"),
            &tx("pause.page.vehicle.action.couple.name"),
            &tx("pause.page.vehicle.action.couple.desc"),
            "couple",
        ));
        fleet.push(button(
            &tx("pause.page.vehicle.action.uncouple.name"),
            &tx("pause.page.vehicle.action.uncouple.name"),
            &tx("pause.page.vehicle.action.uncouple.desc"),
            "uncouple",
        ));
        fleet.push(button(
            &tx("pause.page.vehicle.action.remove.name"),
            &tx("pause.page.vehicle.action.remove.button"),
            &tx("pause.page.vehicle.action.remove.desc"),
            "remove",
        ));
    }
    if !app.placed.is_empty() {
        fleet.push(button(
            &tx("pause.page.vehicle.action.clearplaced.name"),
            &tx("pause.page.vehicle.action.remove.button"),
            &tx("pause.page.vehicle.action.clearplaced.desc"),
            "clearplaced",
        ));
    }
    let mut service: Vec<(String, String)> = Vec::new();
    if has {
        service.push(button(
            &tx("pause.page.vehicle.action.refuel.name"),
            &tx("pause.page.vehicle.action.refuel.name"),
            &tx("pause.page.vehicle.action.refuel.desc"),
            "refuel",
        ));
        service.push(button(
            &tx("pause.page.vehicle.action.wash.name"),
            &tx("pause.page.vehicle.action.wash.name"),
            &tx("pause.page.vehicle.action.wash.desc"),
            "wash",
        ));
        service.push(button(
            &tx("pause.page.vehicle.action.repair.name"),
            &tx("pause.page.vehicle.action.repair.name"),
            &tx("pause.page.vehicle.action.repair.desc"),
            "repair",
        ));
        service.push(button(
            &tx("pause.page.vehicle.action.reset_vehicle.name"),
            &tx("pause.page.vehicle.action.reset_vehicle.button"),
            &tx("pause.page.vehicle.action.reset_vehicle.desc"),
            "reset_vehicle",
        ));
        service.push(button(
            &tx("pause.page.vehicle.action.reload.name"),
            &tx("pause.page.vehicle.action.reload.button"),
            &tx("pause.page.vehicle.action.reload.desc"),
            "reload",
        ));
    }
    let mut driver: Vec<(String, String)> = Vec::new();
    if !server {
        driver.push(opens(
            &tx("pause.page.vehicle.group.driver.title"),
            &tx("pause.page.vehicle.action.driver.desc"),
            "driver",
        ));
    }
    if has && app.on_foot.is_none() {
        driver.push(button(
            &tx("pause.page.vehicle.action.getout.name"),
            &tx("pause.page.vehicle.action.getout.button"),
            &tx("pause.page.vehicle.action.getout.desc"),
            "getout",
        ));
    }
    let mut teleport: Vec<(String, String)> = Vec::new();
    if has && !server && app.navigator.is_some() {
        teleport.push(button(
            &tx("pause.page.vehicle.action.teleport.name"),
            &tx("pause.page.vehicle.action.teleport.button"),
            &tx("pause.page.vehicle.action.teleport.desc"),
            "teleport",
        ));
        teleport.push(opens(
            &tx("pause.page.vehicle.action.tplist.name"),
            &tx("pause.page.vehicle.action.tplist.desc"),
            "tplist",
        ));
    }
    vec![
        (tx("pause.page.vehicle.group.fleet.title"), fleet),
        (tx("pause.page.vehicle.group.display.title"), display),
        (tx("pause.page.vehicle.group.service.title"), service),
        (tx("pause.page.vehicle.group.driver.title"), driver),
        (tx("pause.page.vehicle.group.teleport.title"), teleport),
    ]
}

/// The vehicle page of the pause menu: group ids and the ids of their actions (the same ids
/// `page_action` takes). The texts live in the i18n files under `pause.page.vehicle`.
pub(crate) fn vehicle_menu(app: &App) -> Vec<(&'static str, Vec<(&'static str, bool)>)> {
    let has = app.player.is_some();
    let server = crate::input_script::on_server(&app.args);
    let mut fleet: Vec<(&'static str, bool)> = Vec::new();
    if has || !app.placed.is_empty() {
        fleet.push(("switch", false));
    }
    fleet.push(("place", true));
    if has {
        fleet.extend([
            ("swap", false),
            ("couple", false),
            ("uncouple", false),
            ("remove", false),
        ]);
    }
    if !app.placed.is_empty() {
        fleet.push(("clearplaced", false));
    }
    let mut display: Vec<(&'static str, bool)> = Vec::new();
    if has {
        display.extend([("dest", true), ("hof", true), ("number", true)]);
    }
    let mut service: Vec<(&'static str, bool)> = Vec::new();
    if has {
        service.extend([
            ("refuel", false),
            ("wash", false),
            ("repair", false),
            ("reset_vehicle", false),
            ("reload", false),
        ]);
    }
    let mut driver: Vec<(&'static str, bool)> = Vec::new();
    if !server {
        driver.push(("driver", true));
    }
    if has && app.on_foot.is_none() {
        driver.push(("getout", false));
    }
    let mut teleport: Vec<(&'static str, bool)> = Vec::new();
    if has && !server && app.navigator.is_some() {
        teleport.extend([("teleport", false), ("tplist", true)]);
    }
    vec![
        ("fleet", fleet),
        ("display", display),
        ("service", service),
        ("driver", driver),
        ("teleport", teleport),
    ]
    .into_iter()
    .filter(|g| !g.1.is_empty())
    .collect()
}
