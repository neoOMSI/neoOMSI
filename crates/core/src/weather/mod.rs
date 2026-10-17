pub mod blend;
pub mod editor;
pub mod effects;
pub mod metar;
pub mod model;
pub mod net;
pub mod region;
pub mod sky;
pub mod spec;
pub mod template;

pub use blend::{Blend, Cycle};
pub use editor::{CUSTOM_CLOUDS, CUSTOM_PRECIP, CustomWeather, custom_weather};
pub use effects::{
    apply_to_vehicle, debug_sound_every, enhanced_sky, initial_wetness, lighting, road_wetness, street_condition,
    wind_vector,
};
pub use model::{CloudKind, Haze, Layer, Precip, PrecipKind, Source, Weather};
pub use region::{Local, Regional};
pub use spec::{CYCLE, MetarMode, REPORT, is_cycle, metar_mode};

use crate::cli::Args;

pub fn setup_sky(
    args: &Args,
    renderer: &::render::Renderer,
    scene: &mut ::render::Scene,
    envir: Option<&::content::Envir>,
) {
    const STOCK: [&str; 3] = ["Texture\\himmel01.bmp", "Texture\\himmel04.bmp", "Texture\\himmel05.bmp"];
    let names = envir
        .map(|e| e.sky_textures.clone())
        .unwrap_or_else(|| STOCK.map(String::from));
    let mut ids = Vec::new();
    for (name, stock) in names.iter().zip(STOCK) {
        let path = ::mods::resolve_path(&args.root, name);
        let image = ::texture::decode_file(&path).or_else(|e| {
            log::warn!("sky texture {}: {e}", path.display());
            ::texture::decode_file(&::mods::resolve_path(&args.root, stock))
        });
        match image {
            Ok(img) => ids.push(renderer.add_texture(scene, &img, false)),
            Err(e) => {
                log::warn!("sky texture {stock}: {e}");
                return;
            }
        }
    }
    let started = std::time::Instant::now();
    let field = sky::field();
    log::debug!("cloud field ready after {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
    let clouds = renderer.add_texture(scene, &field.image, true);
    renderer.set_sky_textures_clouds(scene, [ids[0], ids[1], ids[2]], Some(clouds));
}
