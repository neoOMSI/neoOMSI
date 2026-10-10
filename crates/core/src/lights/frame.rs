use super::config::{settings, LightSettings};
use super::tuning::*;
use super::weather::visible_range;
use crate::scene::World;
use glam::DVec3;
use ::simulation::collision::CollisionWorld;
use ::simulation::Daylight;
use std::sync::Arc;

pub(super) struct Frame {
    pub camera: DVec3,

    pub night: f32,

    pub dark: f32,
    pub lamps_on: bool,
    pub visible_range: f64,
    pub cfg: LightSettings,

    pub coll: Arc<CollisionWorld>,

    pub seen: Arc<CollisionWorld>,
    pub generation: u64,
    pub world_id: usize,
    pub index: u64,
}

impl Frame {
    pub(super) fn new(world: &World, daylight: &Daylight, camera: DVec3, index: u64) -> Self {
        let night = daylight.night;
        Self {
            camera,
            night,
            dark: 0.3 + 0.7 * night.clamp(0.0, 1.0),
            lamps_on: daylight.lamps_on,
            visible_range: visible_range(),
            cfg: settings(),
            coll: world.collision.lock().clone(),
            seen: world.light_occluders.lock().clone(),
            generation: world
                .tiles_generation
                .load(std::sync::atomic::Ordering::Relaxed),
            world_id: world as *const World as usize,
            index,
        }
    }

    pub(super) fn dist(&self, p: DVec3) -> f64 {
        (p - self.camera).length()
    }

    pub(super) fn light_range(&self) -> f64 {
        MAP_LIGHT_RANGE.min(self.visible_range)
    }
}
