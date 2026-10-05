use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VehicleKind {
    /// `.bus`
    #[default]
    Bus,
    /// `.ovh` `[type]` 0 = car/other road vehicle, 1 = ?, 2 = rail, 3 = aircraft/helicopter…
    Other(i32),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Camera {
    pub pos: [f32; 3],
    pub dist: f32,
    pub fov: f32,
    pub yaw: f32,
    pub pitch: f32,
    /// `[add_camera_reflexion_2]` extra parameter.
    pub extra: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Axle {
    pub long: f32,
    pub max_width: f32,
    pub min_width: f32,
    pub wheel_diameter: f32,
    pub spring: f32,
    pub max_force: f32,
    pub damper: f32,
    pub driven: bool,
    pub inertia_inv: f32,
}

impl Default for Axle {
    fn default() -> Self {
        Self {
            long: 0.0,
            max_width: 2.0,
            min_width: 1.5,
            wheel_diameter: 1.0,
            spring: 0.0,
            max_force: 0.0,
            damper: 0.0,
            driven: false,
            inertia_inv: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Attachment {
    pub ops: Vec<(String, Vec<f32>)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScriptSet {
    pub varlists: Vec<PathBuf>,
    pub stringvarlists: Vec<PathBuf>,
    pub scripts: Vec<PathBuf>,
    pub constfiles: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Coupling {
    pub pos: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ControlCable {
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Vehicle {
    pub path: PathBuf,
    pub kind: VehicleKind,
    pub manufacturer: String,
    pub type_name: String,
    pub default_paint: String,
    /// The file has a `[friendlyname]`: OMSI offers exactly these in its vehicle list.
    /// Rear sections of articulated buses, trailers and most AI-only variants have none.
    pub has_friendly_name: bool,
    pub friendly_name_inv: Vec<String>,
    pub description: String,
    pub ai_veh_type: i32,
    pub number_file: Option<String>,
    /// `[registration_automatic]`: prefix and postfix around the fleet number.
    pub registration_automatic: Option<(String, String)>,
    /// `[registration_list]`: the file of plates (line by line beside the `[number]` list),
    /// then the prefix and postfix for a number the file has no plate for.
    pub registration_list: Option<(String, String, String)>,
    pub registration_free: bool,
    /// The plate mode the last of those keywords set (TRoadVehicle +0x28d): 0 none, 1 free,
    /// 2 list, 3 automatic; and the prefix and postfix the list and automatic modes share
    /// (+0x2a4, +0x2a8: the later keyword's).
    pub registration_mode: u8,
    pub registration_affix: (String, String),
    pub km_counter_init: Option<(i32, f32)>,
    pub sound: Option<String>,
    pub sound_ai: Option<String>,
    pub model: Option<String>,
    pub paths: Option<String>,
    pub passenger_cabin: Option<String>,
    pub scripts: ScriptSet,
    pub script_share: bool,
    pub cameras_driver: Vec<Camera>,
    pub cameras_pax: Vec<Camera>,
    pub cameras_reflexion: Vec<Camera>,
    pub view_schedule: Option<usize>,
    pub view_ticketselling: Option<usize>,
    pub camera_std: usize,
    pub camera_outside_center: [f32; 3],
    pub mass: f32,
    pub moment_of_inertia: [f32; 3],
    pub bounding_box: Option<[f32; 6]>,
    /// `[cog]`: centre of gravity (x right, y forward, z up) of the physics object.
    pub cog: Option<[f32; 3]>,
    /// `[schwerpunkt]`: height of the centre of gravity.
    pub cog_height: f32,
    pub rolling_resistance: f32,
    pub rot_pnt_long: f32,
    pub inv_min_turn_radius: f32,
    pub ai_delta_height: f32,
    pub axles: Vec<Axle>,
    pub attachments: Vec<Attachment>,
    pub coupling_front: Option<Coupling>,
    pub coupling_back: Option<Coupling>,
    pub couple_front: Option<(String, bool)>,
    pub couple_back: Option<(String, bool)>,
    pub couple_front_open_for_sound: bool,
    pub coupling_front_character: Option<[f32; 4]>,
    pub control_cable_front: Vec<ControlCable>,
    pub control_cable_back: Vec<ControlCable>,
    pub rowdy_factor: Option<(f32, f32)>,
    pub boogies: Option<f32>,
    pub sinus: Option<[f32; 4]>,
    pub rail_body_osc: Option<[f32; 7]>,
    pub contact_shoes: Vec<[f32; 6]>,
    pub ai_brake_performance: Option<[f32; 5]>,
    pub fixed: bool,
    pub unknown_keywords: Vec<(String, usize)>,
}

impl Vehicle {
    pub fn load(path: &Path) -> Result<Vehicle, omsi_cfg::CfgError> {
        let f = CfgFile::read(path)?;
        Ok(Self::parse(&f))
    }

    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new(""))
    }

    /// Whether the vehicle is offered for driving. OMSI 2 lists the `.bus`/`.ovh` files
    /// that carry a `[friendlyname]` (every stock bus does; the GN92's rear section, most
    /// `_KI` AI variants and the AI cars do not), so a rear section never shows up as a
    /// bus of its own.
    pub fn is_selectable(&self) -> bool {
        self.has_friendly_name
    }

    /// The file of the vehicle coupled behind this one (`[couple_back]`), wherever it lives.
    pub fn couple_back_path(&self) -> Option<PathBuf> {
        self.couple_back
            .as_ref()
            .map(|(f, _)| omsi_cfg::resolve_path(self.dir(), f))
    }

    /// A part that is only ever coupled behind another vehicle: the rear section of an
    /// articulated bus (it has a front coupling and no name of its own).
    pub fn is_rear_section(&self) -> bool {
        self.coupling_front.is_some() && !self.has_friendly_name
    }
}
