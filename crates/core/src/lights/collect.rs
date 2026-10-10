use super::cones;
use super::fader::Faders;
use super::frame::Frame;
use super::map_lights::{NearCache, ObjectLampVis};
use super::occluders::{self, OccluderCache};
use super::particles::particle_sprites;
use super::screens;
use super::vehicle::{self, VehicleCache};
use crate::scene::World;
use glam::DVec3;
use ::render::{Corona, LightMode, PointLight, Scene};
use ::simulation::{Daylight, VehicleInstance};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

#[derive(Default)]
struct State {
    frame: u64,
    last: Option<Instant>,
    last_log: Option<Instant>,
    faders: Faders,
    near: NearCache,
    objects: ObjectLampVis,
    vehicles: VehicleCache,
    occluders: OccluderCache,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);

pub fn collect(
    world: &World,
    scene: &mut Scene,
    daylight: &Daylight,
    camera_pos: DVec3,
    vehicles: &[&VehicleInstance],
) {
    let started = Instant::now();
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let st = &mut *guard;
    st.frame += 1;
    let dt = st.last.map_or(0.016, |t| started.duration_since(t).as_secs_f32());
    st.last = Some(started);
    st.faders.begin_frame(dt);

    scene.lights.clear();
    scene.coronas.clear();
    scene.smoke.clear();
    ::simulation::particles::set_eye(camera_pos);
    let f = Frame::new(world, daylight, camera_pos, st.frame);

    st.near.update(&f, world);
    st.near.emit(&f, scene);
    st.objects.emit(&f, world, scene);
    for list in world.particle_objects.lock().values() {
        for po in list {
            if f.dist(po.pos) < f.visible_range {
                particle_sprites(&po.set, false, &mut scene.smoke, &mut scene.coronas);
            }
        }
    }
    let t_map = Instant::now();

    vehicle::emit_all(&f, &mut st.faders, &mut st.vehicles, scene, vehicles);
    cones::shape(scene, f.cfg.corona);
    screens::emit(&f, scene, vehicles);
    let t_vehicles = Instant::now();

    if ::legacy_config::env::var_os("OMSI_DEBUG_LIGHT").is_some() {
        debug_light(scene, camera_pos);
    }

    scene
        .lights
        .sort_by_cached_key(|l| ((l.position - camera_pos).length_squared() * 16.0) as u64);
    occluders::assign(&f, &mut st.occluders, scene, vehicles);

    let total = started.elapsed();
    if total.as_millis() > 10 && st.last_log.map_or(true, |t| t.elapsed().as_secs_f32() > 2.0) {
        st.last_log = Some(Instant::now());
        log::info!(
            "lights.collect {:.0} ms: map {:.0}, vehicles+screens {:.0}, occluders {:.0} ({} lights, {} coronas, {} occluders, {} vehicles)",
            total.as_secs_f64() * 1e3,
            (t_map - started).as_secs_f64() * 1e3,
            (t_vehicles - t_map).as_secs_f64() * 1e3,
            t_vehicles.elapsed().as_secs_f64() * 1e3,
            scene.lights.len(),
            scene.coronas.len(),
            scene.occluders.len(),
            vehicles.len()
        );
    }
}

fn debug_light(scene: &mut Scene, camera: DVec3) {
    scene.lights.push(PointLight {
        position: camera + DVec3::new(0.0, 15.0, -2.0),
        radius: 40.0,
        color: [1.0, 0.9, 0.7],
        intensity: 2.0,
        mode: LightMode::Both,
        ..Default::default()
    });
    scene.coronas.push(Corona {
        position: camera + DVec3::new(0.0, 15.0, 0.0),
        size: 1.0,
        color: [1.0, 0.9, 0.7],
        brightness: 1.0,
        ..Default::default()
    });
}
