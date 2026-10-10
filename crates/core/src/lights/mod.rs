mod beam;
mod collect;
mod config;
mod cones;
mod fader;
mod frame;
mod geom;
mod map_lights;
mod occluders;
mod particles;
mod screens;
mod sources;
mod spill;
mod spot2;
mod sprites;
mod tuning;
mod vehicle;
mod weather;

pub use collect::collect;
pub(crate) use config::{
    exterior_cfg, interior_cfg, reset_exterior_cfg, reset_interior_cfg, set_exterior_cfg,
    set_interior_cfg, set_settings, settings, BeamCfg, ExteriorCfg, InteriorCfg, LampLightCfg,
    LightSettings, MapSpotCfg, SourceCfg, SpillCfg, Spot2Cfg,
};
pub use config::{reset_screen_fx, screen_fx, set_led_glow, set_screen_fx};
pub use sprites::{
    cone_texture_id, corona_texture_id, glow_texture_id, load_smoke_texture, set_corona_root,
    star_texture_id, upload_corona_textures,
};
pub use weather::{apply_weather, lighting_from, set_cone_strength};

fn owner_key<T>(t: &T) -> usize {
    t as *const T as usize
}

pub fn vehicle_velocity(v: &::simulation::VehicleInstance) -> glam::Vec3 {
    let h = v.heading.to_radians();
    glam::Vec3::new(h.sin() as f32, h.cos() as f32, 0.0) * v.physics.speed
}
