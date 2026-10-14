use super::frame::Frame;
use super::geom::{cell3, enclosure, sees};
use super::sources::corona_lights;
use super::tuning::*;
use crate::scene::{LightSwitch, World};
use ::render::{Corona, LightMode, PointLight, Scene};
use ::simulation::collision::CollisionWorld;
use glam::DVec3;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

#[derive(Clone, Copy, PartialEq)]
struct BuildKey {
    world: usize,
    generation: u64,
    lamps_on: bool,
}

/// Near lamps sized against the buildings on a worker thread (by a dense mesh, 20 ms a lamp).
struct Build {
    key: BuildKey,
    centre: DVec3,
    counts: (usize, usize),
    cancel: Arc<AtomicBool>,
    worker: JoinHandle<(Vec<PointLight>, Vec<Corona>)>,
}

#[derive(Default)]
struct Sweep {
    eye: DVec3,
    centre: DVec3,
    cursor: usize,
    valid: bool,
}

#[derive(Default)]
pub(super) struct NearCache {
    key: Option<BuildKey>,
    centre: DVec3,
    counts: (usize, usize),
    lights: Vec<PointLight>,
    coronas: Vec<Corona>,
    light_vis: Vec<bool>,
    corona_vis: Vec<bool>,
    build: Option<Build>,
    sweep: Sweep,
}

impl NearCache {
    pub(super) fn update(&mut self, f: &Frame, world: &World) {
        let static_lights = world.static_lights.lock();
        let static_coronas = world.static_coronas.lock();
        let key = BuildKey {
            world: f.world_id,
            generation: f.generation,
            lamps_on: f.lamps_on,
        };
        let counts = (static_lights.len(), static_coronas.len());
        let stale =
            self.key != Some(key) || self.counts != counts || f.dist(self.centre) > NEAR_MARGIN;

        if let Some(b) = self.build.take_if(|b| b.key != key) {
            b.cancel.store(true, Ordering::Relaxed);
        }
        if stale && self.build.is_none() {
            self.build = Some(Self::start_build(
                f,
                key,
                counts,
                &static_lights,
                &static_coronas,
            ));
        }
        drop((static_lights, static_coronas));

        if let Some(b) = self.build.take_if(|b| b.worker.is_finished()) {
            if let Ok((lights, coronas)) = b.worker.join() {
                *self = NearCache {
                    key: Some(b.key),
                    centre: b.centre,
                    counts: b.counts,
                    lights,
                    coronas,
                    ..Default::default()
                };
            }
        }
        self.sweep_visibility(f);
    }

    fn start_build(
        f: &Frame,
        key: BuildKey,
        counts: (usize, usize),
        lights: &[PointLight],
        coronas: &[crate::scene::StaticCorona],
    ) -> Build {
        let src_lights = if f.lamps_on {
            lights
                .iter()
                .filter(|l| f.dist(l.position) < MAP_LIGHT_RANGE + NEAR_MARGIN)
                .copied()
                .collect()
        } else {
            Vec::new()
        };
        let src_coronas = coronas
            .iter()
            .filter_map(|c| {
                let on = match &c.switch {
                    LightSwitch::Constant(x) => *x,
                    LightSwitch::Night | LightSwitch::Variable(_) => f.lamps_on as i32 as f32,
                };
                if on <= 0.0 || f.dist(c.corona.position) > CORONA_RANGE + NEAR_MARGIN {
                    return None;
                }
                let mut corona = c.corona;
                corona.brightness *= on.min(1.0);
                Some(corona)
            })
            .collect();
        let cancel = Arc::new(AtomicBool::new(false));
        let (coll, stop) = (f.coll.clone(), cancel.clone());
        let worker = std::thread::Builder::new()
            .name("lamp build".into())
            .spawn(move || build(&coll, src_lights, src_coronas, &stop))
            .expect("lamp build thread");
        Build {
            key,
            centre: f.camera,
            counts,
            cancel,
            worker,
        }
    }

    fn sweep_visibility(&mut self, f: &Frame) {
        self.light_vis.resize(self.lights.len(), true);
        self.corona_vis.resize(self.coronas.len(), true);
        let total = self.lights.len() + self.coronas.len();
        let sw = &mut self.sweep;
        if !sw.valid || (sw.cursor >= total && f.dist(sw.centre) > OCC_RECHECK) {
            *sw = Sweep {
                eye: f.camera,
                centre: f.camera,
                cursor: 0,
                valid: true,
            };
        }
        let nl = self.lights.len();
        let mut budget = VIS_PER_FRAME;
        while sw.cursor < total && budget > 0 {
            let i = sw.cursor;
            if i < nl {
                let p = self.lights[i].position;
                self.light_vis[i] =
                    (p - sw.eye).length() > OCC_LIGHT_RANGE || sees(&f.coll, sw.eye, p);
            } else {
                let p = self.coronas[i - nl].position;
                self.corona_vis[i - nl] =
                    (p - sw.eye).length() > OCC_CORONA_RANGE || sees(&f.coll, sw.eye, p);
            }
            sw.cursor += 1;
            budget -= 1;
        }
    }

    fn visible_lights(&self) -> impl Iterator<Item = &PointLight> {
        self.lights
            .iter()
            .zip(&self.light_vis)
            .filter(|(_, v)| **v)
            .map(|(l, _)| l)
    }

    fn visible_coronas(&self) -> impl Iterator<Item = &Corona> {
        self.coronas
            .iter()
            .zip(&self.corona_vis)
            .filter(|(_, v)| **v)
            .map(|(c, _)| c)
    }

    pub(super) fn emit(&self, f: &Frame, scene: &mut Scene) {
        let range2 = f.light_range().powi(2);
        let spot = f.cfg.map_spot;
        scene.lights.extend(
            self.visible_lights()
                .filter(|l| (l.position - f.camera).length_squared() < range2)
                .map(|l| spot.apply(*l)),
        );
        self.emit_street_lamps(f, scene);
        let vis2 = f.visible_range * f.visible_range;
        scene.coronas.extend(
            self.visible_coronas()
                .filter(|c| (c.position - f.camera).length_squared() <= vis2)
                .copied(),
        );
    }

    fn emit_street_lamps(&self, f: &Frame, scene: &mut Scene) {
        let cfg = f.cfg.lamp_light;
        let max = cfg.max.max(0) as usize;
        if !cfg.on || cfg.gain <= 0.0 || max == 0 {
            return;
        }
        let reach = f.light_range();
        let mut lamps: Vec<(f64, &Corona)> = self
            .visible_coronas()
            .filter(|c| !c.beam && !c.halo && c.flags & 8 == 0 && c.brightness > 0.0)
            .map(|c| (f.dist(c.position), c))
            .filter(|(d, _)| *d < reach)
            .collect();

        let keep = (max * 2).min(lamps.len());
        if keep == 0 {
            return;
        }
        if keep < lamps.len() {
            lamps.select_nth_unstable_by(keep - 1, |a, b| a.0.total_cmp(&b.0));
            lamps.truncate(keep);
        }
        lamps.sort_by(|a, b| a.0.total_cmp(&b.0));

        let radius = cfg.range.max(0.5);
        let mut seen: std::collections::HashSet<[i64; 3]> = Default::default();
        for (_, c) in lamps {
            if !seen.insert(cell3(c.position, 10.0)) {
                continue;
            }
            if seen.len() > max {
                break;
            }
            let light = PointLight {
                position: c.position,
                radius,
                color: c.color,
                intensity: c.brightness.min(2.0) * cfg.gain * f.dark,
                core: cfg.core.min(radius),
                mode: LightMode::Both,
                ..Default::default()
            };
            scene.lights.push(f.cfg.map_spot.apply(light));
        }
    }
}

fn build(
    coll: &CollisionWorld,
    src_lights: Vec<PointLight>,
    src_coronas: Vec<Corona>,
    cancel: &AtomicBool,
) -> (Vec<PointLight>, Vec<Corona>) {
    let mut lights = Vec::with_capacity(src_lights.len());
    for mut l in src_lights {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if let Some(extent) = enclosure(coll, l.position) {
            l.radius = l.radius.min((extent + 1.0).max(ENCL_MIN_RADIUS));
            l.core = l.core.min(l.radius);
        }
        lights.push(l);
    }
    let coronas = src_coronas
        .into_iter()
        .take_while(|_| !cancel.load(Ordering::Relaxed))
        .filter(|c| enclosure(coll, c.position).is_none())
        .collect();
    (lights, coronas)
}

#[derive(Default)]
pub(super) struct ObjectLampVis {
    centre: Option<DVec3>,
    epoch: u32,
    seen: HashMap<[i64; 3], (bool, u32)>,
}

impl ObjectLampVis {
    fn new_epoch_if_moved(&mut self, f: &Frame) {
        if self.centre.map_or(true, |c| f.dist(c) > OCC_RECHECK) {
            self.centre = Some(f.camera);
            self.epoch = self.epoch.wrapping_add(1);
            if self.seen.len() > 20_000 {
                self.seen.clear();
            }
        }
    }

    pub(super) fn emit(&mut self, f: &Frame, world: &World, scene: &mut Scene) {
        self.new_epoch_if_moved(f);
        let mut rays = LAMP_RAYS_PER_FRAME;
        let mut with_light: Vec<Corona> = Vec::new();
        for lamp in world.light_objects.lock().iter() {
            let lamp = (lamp.pos, &lamp.coronas[..], &lamp.lit[..]);
            self.emit_object(f, lamp, &mut rays, scene, &mut with_light);
        }
        let scripted = world.scripted.lock();
        for o in scripted.iter().filter(|o| !o.coronas.is_empty()) {
            let lamp = (o.pos, &o.coronas[..], &o.lit[..]);
            self.emit_object(f, lamp, &mut rays, scene, &mut with_light);
        }
        with_light.sort_by(|a, b| f.dist(a.position).total_cmp(&f.dist(b.position)));
        with_light.truncate(SRC_MAX_OBJECTS * 3);
        corona_lights(
            &f.cfg,
            &with_light,
            f.dark,
            SRC_MAX_OBJECTS,
            &mut scene.lights,
        );
    }

    /// An object's lit coronas, unless it is out of sight.
    fn emit_object(
        &mut self,
        f: &Frame,
        (pos, coronas, lit): (DVec3, &[(Corona, String)], &[f32]),
        rays: &mut usize,
        scene: &mut Scene,
        with_light: &mut Vec<Corona>,
    ) {
        let dist = f.dist(pos);
        if dist > f.visible_range {
            return;
        }
        if dist < OCC_CORONA_RANGE {
            let epoch = self.epoch;
            let entry = self
                .seen
                .entry(cell3(pos, 2.0))
                .or_insert((true, epoch.wrapping_sub(1)));
            if entry.1 != epoch && *rays > 0 {
                *rays -= 1;
                *entry = (sees(&f.coll, f.camera, pos), epoch);
            }
            if !entry.0 {
                return;
            }
        }
        let first = scene.coronas.len();
        for ((c, _), lit) in coronas.iter().zip(lit) {
            if *lit <= 0.0 {
                continue;
            }
            let mut corona = *c;
            corona.brightness *= lit.min(1.0);
            scene.coronas.push(corona);
        }
        if dist < SRC_RANGE {
            with_light.extend_from_slice(&scene.coronas[first..]);
        }
    }
}
