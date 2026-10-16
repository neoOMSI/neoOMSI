use super::*;

mod camera;
mod controls;
mod doors;
mod drive;
mod ibis;
mod lights;
mod picking;
mod sound;

pub(crate) use camera::*;
pub(crate) use controls::*;
pub(crate) use doors::*;
pub(crate) use drive::*;
pub(crate) use ibis::*;
pub(crate) use picking::*;

pub(crate) struct Player {
    pub(crate) uid: u64,
    pub(crate) vehicle: ::simulation::VehicleInstance,
    pub(crate) render: scene::VehicleRender,
    pub(crate) trailer_renders: Vec<scene::VehicleRender>,
    pub(crate) axes: ::simulation::KeyboardAxes,
    pub(crate) analog: controllers::Analog,
    pub(crate) cam_choice: (usize, usize),
    pub(crate) bindings: Vec<::content::KeyBinding>,
    pub(crate) sounds: Option<::audio::SoundSet>,
    pub(crate) pressed_mesh: Option<usize>,
    pub(crate) pressed_trailer_mesh: Option<(usize, usize)>,
    pub(crate) press_info: (bool, f32),
    pub(crate) auto_drag: Option<AutoDrag>,
    pub(crate) startup: Option<::simulation::startup::StartUp>,
    pub(crate) startup_at: Option<Instant>,
    pub(crate) give_ticket: bool,
    pub(crate) give_change: bool,
    pub(crate) hand_coupled: usize,
    pub(crate) rail_bound: bool,
    pub(crate) rail: Option<rail_drive::RailDrive>,
    pub(crate) cam_before_special: Option<usize>,
    pub(crate) held_keys: hashbrown::HashMap<i32, Vec<String>>,
    pub(crate) head: Vec3,
    pub(crate) head_sim: Vec3,
    pub(crate) head_vel: Vec3,
    pub(crate) head_omega: Vec3,
    pub(crate) steer_look: f32,
    pub(crate) seat: Vec3,
    pub(crate) mirror_offsets: Vec<[f32; 2]>,
    pub(crate) mirrors_dirty: bool,
    pub(crate) take_change: bool,
    pub(crate) toggled_up: hashbrown::HashSet<String>,
    pub(crate) momentary_gears: bool,
    pub(crate) auto_ibis: bool,
    pub(crate) auto_shift: bool,
    pub(crate) auto_shift_wait: f32,
    pub(crate) auto_shift_idle: f32,
    pub(crate) side_lights_by_l: bool,
    pub(crate) driver: Option<driver::DriverFigure>,
    pub(crate) ibis_duty: Option<(String, String, Vec<String>, (usize, String))>,
    pub(crate) ibis_typist: Option<(::simulation::ibis::Typist, String, String, Vec<String>)>,
    pub(crate) duty_typed: bool,
    pub(crate) html_next_stop: Option<usize>,
    pub(crate) ibis_background: bool,
    pub(crate) arm: camera_arm::SpringArm,
    pub(crate) blinker_key_state: u8,
    pub(crate) blinker_cancel: bool,
    /// Last `$msg` from the player's vehicle which reached the HUD.
    pub(crate) message_serial: u64,
}

pub(crate) const ACTION_ALIASES: &[(&str, &[&str])] = &[
    (
        "blinker_left_set",
        &["kw_blinker_links", "blinker_links", "cp_blinker_links"],
    ),
    (
        "blinker_right_set",
        &["kw_blinker_rechts", "blinker_rechts", "cp_blinker_rechts"],
    ),
    (
        "blinker_off",
        &["kw_blinker_aus", "blinker_aus", "cp_blinker_aus"],
    ),
    (
        "blinker_warn_toggle",
        &["kw_blinker_warn", "blinker_warn", "cp_warnblinker_toggle"],
    ),
    (
        "parking_brake_toggle",
        &["parking_brake_mouse", "cp_feststellbremse_toggle"],
    ),
];

fn door_leaf_of(var: &str) -> Option<String> {
    let v = var.to_ascii_lowercase();
    let rest = v
        .strip_prefix("doortarget_")
        .or_else(|| v.strip_prefix("door_"))?;
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    let tail = &rest[digits..];
    (digits > 0 && tail.len() <= 1 && tail.chars().all(|c| c.is_ascii_alphabetic()))
        .then(|| format!("door_{rest}"))
}

#[derive(Debug, Clone)]
pub(crate) struct AutoDrag {
    ev: String,
    var: String,
    target: f32,
    axis: usize,
    sign: f32,
    last: f32,
    stalled: u32,
    t: f32,
}
