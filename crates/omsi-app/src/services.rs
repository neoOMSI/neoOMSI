//! Runs without a window: exporting a bus (`--export-glb`) and the service commands.

use super::*;

/// `--export-glb`: the vehicle alone, no map, written as glTF.
pub(crate) fn run_export(args: &Args, out: &PathBuf) -> Result<()> {
    let bus = args
        .bus
        .clone()
        .ok_or_else(|| anyhow!("--export-glb needs --bus"))?;
    let path = player_bus_path(&args.root, &bus)?;
    let vt = Arc::new(omsi_sim::VehicleType::load(&args.root, &path)?);
    let scheme = paint_scheme(&vt, args.paint.as_deref());
    let mut host = omsi_sim::VehicleHost::new(start_clock(args));
    host.paint_scheme = Some(scheme);
    let mut vehicle = omsi_sim::VehicleInstance::new(vt.clone(), host);
    vehicle.apply_paint_vars(scheme);
    // the coupled rear of an articulated bus
    load_coupled_parts(&args.root, &mut vehicle);
    for _ in 0..3 {
        vehicle.update(1.0 / 30.0);
    }
    export::export_glb(&args.root, &vt, &vehicle, scheme, out)
}

/// Does the vehicle's box overlap one of the loaded `[petrolstation]` objects? That is
/// OMSI's test for the pump, the wash and a repair without travel time.
pub(crate) fn at_petrol_station(world: &World, v: &omsi_sim::VehicleInstance) -> bool {
    let f = crate::lan::footprint_of(v, [2.5, 11.5, 3.0, 0.0, 0.0, 1.5]);
    let me = omsi_sim::collision::Obb {
        center: glam::DVec2::new(f.x, f.y),
        half: glam::DVec2::new(f.width as f64 * 0.5, f.length as f64 * 0.5),
        heading: (f.heading as f64).to_radians(),
        z0: f.z - 3.0,
        z1: f.z + 4.0,
        velocity: glam::DVec2::ZERO,
        mass: 0.0,
        pole: None,
        id: -1,
    };
    let stations = world.petrol_stations.lock();
    if omsi_cfg::env::var_os("OMSI_DEBUG_SERVICES").is_some() {
        for p in stations.iter() {
            log::info!(
                "petrol station box at ({:.1}, {:.1}) {:.1} x {:.1} m: bus {:.1} m away",
                p.center.x,
                p.center.y,
                p.half.x * 2.0,
                p.half.y * 2.0,
                me.separation(p)
            );
        }
    }
    stations.iter().any(|p| me.separation(p) < 0.0)
}

/// The depot services: the fuel pump, the bus wash and the workshop. OMSI offers them
/// from its menu, fires `veh_tank` / `veh_wash` while they run and asks the bus for its
/// repair time with `malfunction_gettime` before it lets the workshop start. The pump and
/// the wash work only at a petrol station (`at_station`); the workshop comes anywhere, and
/// away from one its team needs the map's `[repair_time_min]` to get there
/// (`DG_Repair3`: "Since you are not in the depot the reparation team needs … minutes").
pub(crate) fn run_services(
    args: &Args,
    v: &mut omsi_sim::VehicleInstance,
    clock: &mut omsi_sim::SimClock,
    repair_time_min: f32,
    at_station: bool,
) -> Vec<String> {
    let mut out = Vec::new();
    if (args.refuel || args.wash) && !at_station {
        out.push("Refuel and wash only at a petrol station or in the depot's wash yard".into());
    } else {
        if args.refuel {
            match v.refuel() {
                Some(l) => out.push(format!("refuelled: {l:.0} l in the tank")),
                None => out.push("this vehicle has no fuel pump handling (veh_tank)".into()),
            }
        }
        if args.wash {
            match v.wash() {
                Some(d) => out.push(format!("washed: dirt {:.0}%", d * 100.0)),
                None => out.push("this vehicle has no bus wash handling (veh_wash)".into()),
            }
        }
    }
    if args.repair {
        match v.repair_minutes() {
            Some(mins) => {
                let travel = if at_station { 0.0 } else { repair_time_min };
                clock.time += ((mins + travel) * 60.0) as f64;
                v.repair();
                out.push(if at_station {
                    format!("repaired: {mins:.0} min of work")
                } else {
                    format!(
                        "repaired: {mins:.0} min of work + {travel:.0} min for the team to get here"
                    )
                });
            }
            None => out.push("this vehicle has no repair handling (malfunction_gettime)".into()),
        }
    }
    out
}
