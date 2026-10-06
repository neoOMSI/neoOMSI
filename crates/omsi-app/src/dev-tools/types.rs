use omsi_sim::collision::Obb;

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
    pub cam: Option<omsi_render::Camera>,
    pub foot: Option<FootInfo>,
    pub boxes: Vec<Obb>,
    pub doors: Vec<DoorDbg>,
    pub blockers: Vec<Obb>,
    pub lan: Option<LanInfo>,
    pub tours: Vec<TourRow>,
    pub quicksave: bool,
}

pub(crate) enum Action {
    QuickSave,
    LoadQuickSave,
    CopyCode,
    OpenLan(u16),
    Connect(String),
    Vehicle(String),
    Cockpit(usize),
    VehicleSaloonLights,
    VehicleStartUp,
}
