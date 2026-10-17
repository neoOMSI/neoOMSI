use crate::*;
use ::render::Lighting;

pub(super) fn lighting(args: &Args, weather: &::content::weather::Weather) -> Lighting {
    let clock = start_clock(args);
    let envir = ::content::Envir::load(&args.root.join("envir.cfg")).ok();
    let daylight = ::simulation::Daylight::compute(&clock, envir.as_ref());
    let mut l = weather_lighting(&daylight, weather, cloud_drift_at(weather, clock.time), 0.0, true);
    l.shadows = daylight.altitude_deg > 2.0;
    l.enhanced = false;
    l.classic = false;
    l.detail = false;
    l.wetness = 0.0;
    l.snow = 0.0;
    l.fog_density = 0.0;
    l
}
