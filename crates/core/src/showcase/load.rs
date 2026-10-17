use super::look::Look;
use super::light;
use crate::*;
use ::render::{Lighting, Renderer, Scene};
use std::sync::mpsc::{Receiver, channel};

pub(super) struct Ready {
    look: Look,
    world: Arc<World>,
    vt: Arc<::simulation::VehicleType>,
    vehicle: ::simulation::VehicleInstance,
    scheme: Option<usize>,
}

pub(super) struct Model {
    pub look: Look,
    pub scene: Scene,
    pub vehicle: ::simulation::VehicleInstance,
    pub render: scene::VehicleRender,
    pub trailers: Vec<scene::VehicleRender>,
    pub centre: Vec3,
    pub radius: f32,
    pub lighting: Lighting,
    pub world: Arc<World>,
}

pub(super) type Job = Receiver<Result<Ready, String>>;

pub(super) fn spawn(renderer: &Renderer, look: &Look) -> Result<Job, String> {
    if look.bus.is_empty() {
        return Err("no bus".into());
    }
    let args = look.args();
    let root = look.root.clone();
    let gpu = (renderer.device.clone(), renderer.queue.clone());
    let look = look.clone();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let r = (|| -> Result<Ready> {
            let t0 = std::time::Instant::now();
            let map_cfg = ::mods::resolve_path(&root, &look.map);
            let world = Arc::new(World::open(&root, &map_cfg, start_clock(&args).date_code())?);
            let prefetch = world.vehicle_prefetch_on(gpu);
            let path = player_bus_path(&root, &look.bus)?;
            let vt = Arc::new(::simulation::VehicleType::load(&root, &path)?);
            let paint = Some(look.paint.as_str()).filter(|p| !p.is_empty());
            let scheme = paint_scheme(&vt, paint);
            let host = ::simulation::VehicleHost::new(start_clock(&args));
            let mut vehicle = ::simulation::VehicleInstance::new(vt.clone(), host);
            if let Some(i) = scheme {
                for (var, v) in vt.paint_schemes[i].set_vars.clone() {
                    vehicle.set_var(&var, v);
                }
            }
            load_coupled_parts(&root, &mut vehicle);
            for _ in 0..3 {
                vehicle.update(1.0 / 30.0);
            }
            prefetch.prefetch(&vt, scheme);
            for t in &vehicle.trailers {
                prefetch.prefetch(&t.ty, scheme.filter(|i| *i < t.ty.paint_schemes.len()));
            }
            log::info!("showroom {}: read in {:.2} s", look.bus, t0.elapsed().as_secs_f64());
            Ok(Ready { look, world, vt, vehicle, scheme })
        })();
        let _ = tx.send(r.map_err(|e| format!("{e:#}")));
    });
    Ok(rx)
}

pub(super) fn place(renderer: &Renderer, r: Ready) -> Model {
    let t0 = std::time::Instant::now();
    let mut scene = renderer.new_scene();
    let args = r.look.args();
    let world = r.world;
    let mut vehicle = r.vehicle;
    vehicle.position = DVec3::ZERO;
    vehicle.heading = 0.0;

    let render = world.add_vehicle(renderer, &mut scene, &r.vt, r.scheme);
    let trailers: Vec<scene::VehicleRender> = vehicle
        .trailers
        .iter()
        .map(|t| {
            let scheme = r.scheme.filter(|i| *i < t.ty.paint_schemes.len());
            world.add_vehicle_part(renderer, &mut scene, &t.ty, scheme, &render)
        })
        .collect();

    let decode = |p: &std::path::Path| {
        ::texture::decode_file(p)
            .ok()
            .map(|i| (i.width, i.height, i.rgba))
    };
    vehicle.init_text_textures(&mut world.fonts.lock(), &decode);
    for t in vehicle.trailers.iter_mut() {
        t.init_text_textures(&mut world.fonts.lock(), &decode);
    }
    vehicle.update(1.0 / 30.0);

    let bb = r.vt.def.bounding_box.unwrap_or([2.5, 12.0, 3.0, 0.0, 0.0, 1.5]);
    let front = bb[4] + bb[1] * 0.5;
    let mut total = bb[1];
    for t in &vehicle.trailers {
        let tb = t.ty.def.bounding_box.unwrap_or([2.5, 8.0, 3.0, 0.0, 0.0, 1.5]);
        let behind = (t.position - vehicle.position).truncate().length() as f32 + tb[1] * 0.5;
        total = total.max(bb[1] * 0.5 + behind);
    }
    let centre = Vec3::new(bb[3], front - total * 0.5, bb[5]);
    let radius = 0.5 * (total * total + bb[0] * bb[0] + bb[2] * bb[2]).sqrt();

    let weather = load_weather(&args);
    let lighting = light::lighting(&args, &weather);
    log::info!("showroom {}: placed in {:.2} s", r.look.bus, t0.elapsed().as_secs_f64());
    Model {
        look: r.look,
        scene,
        vehicle,
        render,
        trailers,
        centre,
        radius,
        lighting,
        world,
    }
}
