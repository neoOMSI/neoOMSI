use ::simulation::collision::Obb;

pub(crate) struct Snapshot {
    pub adapter: String,
    pub format: wgpu::TextureFormat,
    pub surface: (u32, u32),
    pub dt_ms: f32,
    pub fps: f32,
    pub msaa: u32,
    pub anisotropy: u16,
    pub shadow_size: u32,
    pub ssao: bool,
    pub fxaa: bool,
    pub reflections: bool,
    pub render_scale: f32,
    pub meshes: usize,
    pub textures: usize,
    pub materials: usize,
    pub instances: usize,
    pub lights: usize,
    pub interior_lights: usize,
    pub coronas: usize,
    pub gpu_passes: Vec<(String, f64, u32)>,
    pub draw_report: Vec<String>,
    pub light_stats: LightStats,
}

#[derive(Clone, Default)]
pub(crate) struct LightStats {
    pub point: usize,
    pub spot: usize,
    pub screen: usize,
    pub shadow_casting: usize,
    pub beams: usize,
    pub vanilla_only: usize,
    pub enhanced_only: usize,
    pub interior_point: usize,
    pub interior_spot: usize,
    pub coronas_beam: usize,
    pub coronas_halo: usize,
    pub coronas_rotating: usize,
    pub occluders: usize,
    pub max_radius: f32,
    pub avg_radius: f32,
    pub total_intensity: f32,
    pub nearest: Option<f32>,
    pub sun_intensity: f32,
    pub sun_elevation: f32,
    pub night: f32,
    pub ambient: f32,
    pub fog_density: f32,
    pub overcast: f32,
    pub rain: f32,
    pub wetness: f32,
    pub snow: f32,
    pub cloud_density: f32,
    pub sun_shadows: bool,
    pub light_shadows: bool,
    pub enhanced: bool,
}

impl LightStats {
    pub fn gather(
        scene: &::render::Scene,
        lighting: &::render::Lighting,
        cam: glam::DVec3,
    ) -> LightStats {
        let mut o = LightStats::default();
        let mut radius_sum = 0.0f32;
        let mut near = f64::MAX;
        for l in &scene.lights {
            if l.is_screen() {
                o.screen += 1;
            } else if l.cone[0] < 0.999 || l.direction.length_squared() > 1e-6 {
                o.spot += 1;
            } else {
                o.point += 1;
            }
            if l.shadow_first {
                o.shadow_casting += 1;
            }
            if l.beam != 0.0 {
                o.beams += 1;
            }
            match l.mode {
                ::render::LightMode::Vanilla => o.vanilla_only += 1,
                ::render::LightMode::Enhanced => o.enhanced_only += 1,
                _ => {}
            }
            o.occluders += l.occ_count as usize;
            o.max_radius = o.max_radius.max(l.radius);
            radius_sum += l.radius;
            o.total_intensity += l.intensity;
            near = near.min((l.position - cam).length());
        }
        for l in &scene.interior_lights {
            if l.direction.length_squared() > 1e-6 {
                o.interior_spot += 1;
            } else {
                o.interior_point += 1;
            }
            o.total_intensity += l.intensity;
        }
        for c in &scene.coronas {
            o.coronas_beam += c.beam as usize;
            o.coronas_halo += c.halo as usize;
            o.coronas_rotating += (c.rotating != 0) as usize;
        }
        let n = scene.lights.len();
        o.avg_radius = if n > 0 { radius_sum / n as f32 } else { 0.0 };
        o.nearest = (near < f64::MAX).then_some(near as f32);
        o.sun_intensity = lighting.sun_intensity;
        o.sun_elevation = lighting
            .sun_dir
            .normalize_or_zero()
            .z
            .clamp(-1.0, 1.0)
            .asin()
            .to_degrees();
        o.night = lighting.night;
        o.ambient = lighting.ambient.length();
        o.fog_density = lighting.fog_density;
        o.overcast = lighting.overcast;
        o.rain = lighting.rain;
        o.wetness = lighting.wetness;
        o.snow = lighting.snow;
        o.cloud_density = lighting.cloud_density;
        o.sun_shadows = lighting.casts_sun_shadows();
        o.light_shadows = lighting.light_shadows;
        o.enhanced = lighting.enhanced;
        o
    }
}

pub(crate) struct FootInfo {
    pub pos: [f64; 3],
    pub heading: f64,
    pub vel: [f64; 2],
    pub vz: f64,
    pub on_lane: bool,
    pub attached: bool,
    pub seated: bool,
    pub inside: bool,
    pub exempt: Option<[f64; 2]>,
    pub solids: usize,
    pub lost: [f64; 2],
    pub step_wall: bool,
    pub reset: bool,
    pub floor: Option<f64>,
    pub lift: f64,
}

pub(crate) struct DoorDbg {
    pub outside: [f64; 3],
    pub inside: [f64; 3],
    pub open: bool,
    pub along: f64,
    pub len: f64,
    pub lateral: f64,
    pub in_lane: bool,
}

pub(crate) struct LanInfo {
    pub host: bool,
    pub connected: bool,
    pub code: Option<String>,
    pub target: Option<String>,
    pub local: Option<String>,
    pub peers: usize,
    pub name: String,
    pub session: u64,
    pub rejected: Option<String>,
    pub sent: u64,
    pub map: String,
}

pub(crate) struct TourRow {
    pub line: String,
    pub number: String,
    pub start: f64,
    pub trips: usize,
    pub available: bool,
}

pub(crate) struct VehicleInfo {
    pub actions: Vec<String>,
    pub controls: Vec<(usize, String)>,
    pub interior: Vec<InteriorInfo>,
    pub exterior: Vec<InteriorInfo>,
    pub walk_points: Vec<[f32; 3]>,
    pub walk_links: Vec<(i32, i32, bool)>,
}

pub(crate) struct InteriorInfo {
    pub variable: String,
    pub pos: [f32; 3],
    pub color: [f32; 3],
    pub range: f32,
}

pub(crate) struct BeamMark {
    pub pos: [f64; 3],
    pub dir: [f32; 3],
    pub cone: bool,
    pub tint: Option<[f32; 3]>,
}

pub(crate) struct Extra {
    /// The driven vehicle's position and body rotation (model frame -> world).
    pub pose: Option<([f64; 3], glam::Mat4)>,
    pub beams: Vec<BeamMark>,
    pub vehicle: Option<VehicleInfo>,
    pub map: String,
    pub clock: f64,
    pub paused: bool,
    pub cam: Option<::render::Camera>,
    pub foot: Option<FootInfo>,
    pub boxes: Vec<Obb>,
    pub doors: Vec<DoorDbg>,
    pub blockers: Vec<Obb>,
    pub lan: Option<LanInfo>,
    pub tours: Vec<TourRow>,
    pub quicksave: bool,
    pub profile: Vec<(&'static str, f64)>,
    pub frames: u32,
    pub traffic: Option<TrafficPerf>,
    pub traffic_debug: Option<crate::traffic::TrafficDebugFrame>,
    pub weather: WeatherInfo,
}

pub(crate) struct WeatherInfo {
    pub spec: String,
    pub weather: ::content::weather::Weather,
    pub custom: crate::weather_setup::CustomWeather,
    pub is_custom: bool,
    pub wetness: f32,
    pub blend: Option<(f32, String)>,
    pub cycle_next: Option<f64>,
    pub client: bool,
    pub metar_locked: bool,
    pub metar_loading: bool,
    pub metar_station: String,
    pub time_locked: bool,
    pub year: i32,
    pub day_of_year: i32,
    pub day_month: (i32, i32),
    pub density: f32,
    pub layers: [[f32; 4]; 3],
    pub precip: (i32, f32),
    pub street_cond: f32,
    pub drift: [f32; 2],
}

pub(crate) struct TrafficPerf {
    pub cars: usize,
    pub dormant: usize,
}

pub(crate) enum Action {
    QuickSave,
    LoadQuickSave,
    CopyCode,
    CopyTrafficReport(String),
    OpenLan(u16),
    Connect(String),
    Vehicle(String),
    Cockpit(usize),
    VehicleSaloonLights,
    VehicleStartUp,
    WeatherPreset(String, f32),
    WeatherNext,
    WeatherCustom(Box<crate::weather_setup::CustomWeather>),
    WeatherFromCurrent,
    WeatherMetar(String),
    WeatherCycle(bool),
    WeatherCycleNow,
    WeatherWetness(f32),
    WeatherSave(String),
    SetTime(f64),
    SetDay(i32),
}
