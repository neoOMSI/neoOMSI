//! Sun position and the three envir.cfg lights (direct sun A, from above B, ambient C)
//! interpolated over the sun altitude (unit `mc_himmel`).

use crate::clock::SimClock;
use glam::Vec3;
use omsi_content::Envir;

#[derive(Debug, Clone, Copy)]
pub struct Daylight {
    /// Unit vector towards the sun (x east, y north, z up).
    pub sun_dir: Vec3,
    pub altitude_deg: f32,
    pub sun_color: Vec3,
    pub secondary: Vec3,
    pub ambient: Vec3,
    pub sky: Vec3,
    /// 0 by day, 1 by night (nightmaps, coronas).
    pub night: f32,
    /// Street lights on (the `NightlightA` scenery variable).
    pub lamps_on: bool,
    /// The daylight by the sun's altitude (1 day … 0 night, a ramp from +6 to -6 degrees):
    /// what the street lamps and the lit windows switch by.
    pub brightness: f32,
    /// The mean of envir.cfg's light A at this altitude (0..1, as Omsi.exe keeps it at
    /// weather +0xac): the day's part of the `Envir_Brightness` vehicle variable, see
    /// [`Daylight::envir_brightness`].
    pub light_a: f32,
    /// Sun azimuth in radians, clockwise from north.
    pub azimuth_rad: f32,
    /// Blend of the envir sky textures: day, twilight (peaks at sunrise), night.
    pub sky_weights: [f32; 3],
    /// envir.cfg's three light colours relative to the stock file's at this sun altitude
    /// (A, B, C; 1 = stock): the enhanced renderer tints its physical sky with them.
    pub envir_tint: [Vec3; 3],
}

/// Where the map lies and how its clocks run, from its `timezone.txt` (OMSI
/// the original: `[timezone]` hours east of UTC, `[location]` latitude and longitude,
/// `[DST]` periods). Without the file the exe keeps Spandau: 52.505 N, 13.2782 E, UTC+1.
#[derive(Debug, Clone, PartialEq)]
pub struct SunPlace {
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: f64,
    /// Daylight saving periods: first and last day (`YYYYMMDD`), the hour it starts on the
    /// first day and ends on the last, and the hours the clocks go forward.
    pub dst: Vec<(i32, i32, f32, f32, f32)>,
}

impl Default for SunPlace {
    fn default() -> Self {
        SunPlace {
            latitude: 52.505,
            longitude: 13.2782,
            timezone: 1.0,
            dst: Vec::new(),
        }
    }
}

impl SunPlace {
    /// The hours the clock is ahead of standard time at this moment (0 outside summer time).
    pub fn dst_hours(&self, clock: &SimClock) -> f64 {
        let date = clock.date_code();
        let hour = clock.time / 3600.0;
        for &(start, end, h0, h1, offset) in &self.dst {
            let after_start = date > start || (date == start && hour >= h0 as f64);
            let before_end = date < end || (date == end && hour < h1 as f64);
            if after_start && before_end {
                return offset as f64;
            }
        }
        0.0
    }

    /// Local solar time (hours) for the map clock: the clock minus its time zone and summer
    /// time is UTC, and the sun stands south at 12:00 plus 4 minutes a degree west.
    pub fn solar_hours(&self, clock: &SimClock) -> f64 {
        clock.time / 3600.0 - self.timezone - self.dst_hours(clock) + self.longitude / 15.0
    }
}

static PLACE: std::sync::RwLock<Option<std::sync::Arc<SunPlace>>> = std::sync::RwLock::new(None);

/// The loaded map's place (see [`SunPlace`]); every sun position is computed for it.
pub fn set_place(p: SunPlace) {
    *PLACE.write().unwrap_or_else(|e| e.into_inner()) = Some(std::sync::Arc::new(p));
}

pub fn place() -> std::sync::Arc<SunPlace> {
    PLACE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default()
}

/// Sun altitude and azimuth (degrees; azimuth clockwise from north) for the map clock at
/// `place`.
pub fn sun_position(clock: &SimClock, place: &SunPlace) -> (f64, f64) {
    let doy = clock.day_of_year as f64;
    let decl =
        (23.44f64).to_radians() * ((2.0 * std::f64::consts::PI * (284.0 + doy) / 365.0).sin());
    let hour = place.solar_hours(clock);
    let h = (15.0 * (hour - 12.0)).to_radians();
    let lat = place.latitude.to_radians();
    let sin_alt = lat.sin() * decl.sin() + lat.cos() * decl.cos() * h.cos();
    let alt = sin_alt.clamp(-1.0, 1.0).asin();
    let cos_az = (decl.sin() - lat.sin() * sin_alt) / (lat.cos() * alt.cos()).max(1e-6);
    let mut az = cos_az.clamp(-1.0, 1.0).acos();
    if h.sin() > 0.0 {
        az = 2.0 * std::f64::consts::PI - az;
    }
    (alt.to_degrees(), az.to_degrees())
}

fn stops(colors: &[[f32; 3]; 5], twilight: (f32, f32), alt: f32) -> Vec3 {
    let alts = [-90.0, twilight.0, 0.0, twilight.1, 90.0];
    let mut i = 0;
    while i + 1 < 4 && alt > alts[i + 1] {
        i += 1;
    }
    let t = ((alt - alts[i]) / (alts[i + 1] - alts[i]).max(1e-3)).clamp(0.0, 1.0);
    let a = Vec3::from(colors[i]);
    let b = Vec3::from(colors[i + 1]);
    a.lerp(b, t) / 255.0
}

impl Daylight {
    pub fn compute(clock: &SimClock, envir: Option<&Envir>) -> Daylight {
        let (alt, az) = sun_position(clock, &place());
        let (sa, ca) = (alt.to_radians().sin() as f32, alt.to_radians().cos() as f32);
        let (saz, caz) = (az.to_radians().sin() as f32, az.to_radians().cos() as f32);
        let sun_dir = Vec3::new(ca * saz, ca * caz, sa).normalize_or_zero();
        let alt = alt as f32;
        let default_envir;
        let envir = match envir {
            Some(e) => e,
            None => {
                default_envir = default_envir_values();
                &default_envir
            }
        };
        let tw = envir.twilight_start_end;
        let a = stops(&envir.light_color_a, tw, alt);
        let b = stops(&envir.light_color_b, tw, alt);
        let c = stops(&envir.light_color_c, tw, alt);
        // scaled so that a sunlit white surface does not blow out
        let sun_color = a * 0.7;
        let secondary = b * 0.5;
        let ambient = (c * 0.45).max(Vec3::splat(0.05));
        // the sky follows the light from above; deep blue at day, near black at night
        let day = ((alt + 6.0) / 16.0).clamp(0.0, 1.0);
        let sky = Vec3::new(0.55, 0.70, 0.92) * day
            + Vec3::new(0.01, 0.01, 0.03) * (1.0 - day)
            + (a * 0.15 * (1.0 - day) * day * 4.0).min(Vec3::splat(0.3));
        let night = ((tw.1 - alt) / tw.1.max(1.0)).clamp(0.0, 1.0);
        let sky_weights = if alt >= tw.1 {
            [1.0, 0.0, 0.0]
        } else if alt >= 0.0 {
            let t = alt / tw.1.max(0.1);
            [t, 1.0 - t, 0.0]
        } else if alt >= tw.0 {
            let t = (alt - tw.0) / (0.0 - tw.0).max(0.1);
            [0.0, t, 1.0 - t]
        } else {
            [0.0, 0.0, 1.0]
        };
        let stock = default_envir_values();
        let ratio = |own: Vec3, base: Vec3| -> Vec3 {
            // where the stock light is (nearly) black there is nothing to compare with
            let r = |o: f32, b: f32| {
                if b > 2.0 / 255.0 {
                    (o / b).clamp(0.25, 4.0)
                } else {
                    1.0
                }
            };
            Vec3::new(r(own.x, base.x), r(own.y, base.y), r(own.z, base.z))
        };
        let envir_tint = [
            ratio(
                a,
                stops(&stock.light_color_a, stock.twilight_start_end, alt),
            ),
            ratio(
                b,
                stops(&stock.light_color_b, stock.twilight_start_end, alt),
            ),
            ratio(
                c,
                stops(&stock.light_color_c, stock.twilight_start_end, alt),
            ),
        ];
        // the street lamps come on below a light value of 0.6, as Omsi.exe switches them
        // (FUN_006ff1bc), the same value at which a scenery object's NightlightA does
        let brightness = ((alt + 6.0) / 12.0).clamp(0.0, 1.0);
        let light_a = ((a.x + a.y + a.z) / 3.0).clamp(0.0, 1.0);
        Daylight {
            sun_dir,
            altitude_deg: alt,
            sun_color,
            secondary,
            ambient,
            sky,
            night,
            lamps_on: brightness < 0.6,
            brightness,
            light_a,
            azimuth_rad: az.to_radians() as f32,
            sky_weights,
            envir_tint,
        }
    }
}

impl Daylight {
    /// The `Envir_Brightness` vehicle variable as Omsi.exe sets it for every road vehicle
    /// (0x7d8735): the mean of the light the tile's light map throws on the vehicle (its
    /// colour at the vehicle's place while the lamps are on, 0x61378c) plus the mean of
    /// light A, at most 1. The stock buses scale their windows' alpha by it (`[alphascale]
    /// Envir_Brightness` on Fenster_braun.tga and the like): by the sun's ramp alone it was
    /// 0 at every night, and a bus under the street lamps had no glass at all (#624).
    pub fn envir_brightness(&self, light_map: Option<Vec3>) -> f32 {
        let lm = light_map
            .filter(|_| self.lamps_on)
            .map(|c| (c.x + c.y + c.z) / 3.0)
            .unwrap_or(0.0);
        (self.light_a + lm.max(0.0)).clamp(0.0, 1.0)
    }
}

fn default_envir_values() -> Envir {
    Envir {
        twilight_start_end: (-18.0, 10.0),
        light_color_a: [
            [0.0; 3],
            [0.0; 3],
            [200.0, 100.0, 20.0],
            [255.0, 255.0, 240.0],
            [255.0; 3],
        ],
        light_color_b: [
            [5.0, 5.0, 10.0],
            [10.0, 10.0, 20.0],
            [30.0, 20.0, 40.0],
            [60.0, 70.0, 80.0],
            [80.0, 90.0, 100.0],
        ],
        light_color_c: [
            [2.0, 2.0, 5.0],
            [15.0, 15.0, 25.0],
            [40.0, 50.0, 60.0],
            [220.0; 3],
            [230.0, 230.0, 255.0],
        ],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock_at(hours: f64) -> SimClock {
        SimClock {
            day_of_year: 182,
            time: hours * 3600.0,
            ..Default::default()
        }
    }

    #[test]
    fn the_sun_follows_the_maps_time_zone_and_summer_time() {
        let mut spandau = SunPlace::default();
        spandau.dst.push((19890326, 19890924, 2.0, 3.0, 1.0));
        // 21 June 1989: summer time, the sun due south at about 13:07 local
        let mut c = SimClock {
            year: 1989,
            day_of_year: 172,
            time: 13.12 * 3600.0,
            ..Default::default()
        };
        let (alt, az) = sun_position(&c, &spandau);
        assert!(
            (az - 180.0).abs() < 3.0 && (alt - 61.0).abs() < 1.5,
            "summer noon: alt {alt:.1} az {az:.1}"
        );
        // 21 December: standard time, south at about 12:07
        c.day_of_year = 355;
        c.time = 12.12 * 3600.0;
        let (alt, az) = sun_position(&c, &spandau);
        assert!(
            (az - 180.0).abs() < 3.0 && (alt - 14.0).abs() < 1.5,
            "winter noon: alt {alt:.1} az {az:.1}"
        );
        // without summer time the June sun would be an hour further on
        c.day_of_year = 172;
        c.time = 13.12 * 3600.0;
        let (_, az) = sun_position(&c, &SunPlace::default());
        assert!(az > 195.0, "no DST: az {az:.1}");
    }

    #[test]
    fn envir_brightness_is_light_a_plus_the_light_map_under_the_lamps() {
        // a December midnight: no light A, so only the light map's light counts
        let night = Daylight::compute(
            &SimClock {
                day_of_year: 355,
                time: 0.0,
                ..Default::default()
            },
            None,
        );
        assert!(night.lamps_on);
        assert_eq!(night.envir_brightness(None), 0.0);
        let lit = night.envir_brightness(Some(Vec3::new(0.6, 0.5, 0.4)));
        assert!((lit - 0.5).abs() < 1e-5, "{lit}");
        // noon: light A alone is (nearly) 1, and the light map is off by day
        let noon = Daylight::compute(&clock_at(12.0), None);
        assert!(noon.envir_brightness(None) > 0.95);
        assert_eq!(
            noon.envir_brightness(Some(Vec3::ONE)),
            noon.envir_brightness(None)
        );
    }

    #[test]
    fn stock_envir_leaves_the_enhanced_sky_untinted() {
        let own = default_envir_values();
        for h in [3.0, 6.0, 12.0, 19.0, 23.0] {
            assert_eq!(
                Daylight::compute(&clock_at(h), None).envir_tint,
                [Vec3::ONE; 3],
                "{h} h"
            );
            assert_eq!(
                Daylight::compute(&clock_at(h), Some(&own)).envir_tint,
                [Vec3::ONE; 3],
                "{h} h"
            );
        }
    }

    #[test]
    fn a_warmer_envir_tints_the_sun() {
        let mut warm = default_envir_values();
        // the midday sun as warm as a late afternoon's
        warm.light_color_a[4] = [255.0, 230.0, 170.0];
        warm.light_color_a[3] = [255.0, 230.0, 160.0];
        let d = Daylight::compute(&clock_at(12.0), Some(&warm));
        let a = d.envir_tint[0];
        assert!((a.x - 1.0).abs() < 1e-3 && a.y < 0.95 && a.z < 0.7, "{a:?}");
        assert_eq!(d.envir_tint[1], Vec3::ONE);
        // by night the stock sun is black: nothing to compare with, no tint
        assert_eq!(
            Daylight::compute(&clock_at(1.0), Some(&warm)).envir_tint[0],
            Vec3::ONE
        );
    }
}
