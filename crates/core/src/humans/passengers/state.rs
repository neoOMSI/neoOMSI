use super::{BusId, DVec3, HashSet, Vec3};

/// Omsi.exe's tasks (+0x6c5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::humans) enum Task {
    Nothing,
    WaitingForBus,
    /// Task 2: the bus comes, to the stop's gather point.
    ToBus,
    WalkingToBus,
    InBusToPlace,
    InBusToExit,
    WalkingToBusstop,
    SittingInBus,
    /// Host-owned until its client confirms the transfer.
    AwaitingTransfer,
}

impl Task {
    pub(in crate::humans) fn name(self) -> &'static str {
        match self {
            Task::Nothing => "DoNothing",
            Task::WaitingForBus => "WaitingForBus",
            Task::ToBus => "BusComing",
            Task::WalkingToBus => "WalkingToBus",
            Task::InBusToPlace => "WalkingInBusToPlace",
            Task::InBusToExit => "WalkingInBusToExit",
            Task::WalkingToBusstop => "WalkingToBusstop",
            Task::SittingInBus => "SittingInBus",
            Task::AwaitingTransfer => "AwaitingTransfer",
        }
    }
}

/// One passenger's state: the fields of the original's human the tasks use.
#[derive(Debug, Clone)]
pub(in crate::humans) struct Pax {
    /// Receipt for a grant accepted in the current LAN session, distinct from a local id.
    pub accepted_transfer: Option<u64>,
    pub seat_approach: Option<SeatApproach>,
    pub seat_floor: Option<SeatFloor>,
    pub doorway: Option<Doorway>,
    pub task: Task,
    /// Movement state +0x6c4: 0 stand, 1 to the target, 2 0.7 m short of it, 3 there, 5
    /// along the paths, 6 0.7 m short of the path's end, 7 at the path's end, 9 turning on
    /// the spot.
    pub movement: Movement,
    /// Inside a bus (+0x5ef clear): `pos` and `yaw` are in its frame.
    pub inside: Option<BusId>,
    /// Where the feet are (the world, or the bus frame), and the heading (radians, 0 =
    /// forward, clockwise from above: Direct3D's yaw).
    pub pos: DVec3,
    pub yaw: f64,
    /// The bus dealt with (+0x6b4), the stop (+0x6bc), the waiting place there (+0x618).
    /// Bus being approached or ridden. A reservation does not imply physical boarding;
    /// `inside` alone selects the coordinate frame and physical occupancy.
    pub bus: Option<BusId>,
    /// The bus awaiting this passenger's LAN transfer acknowledgement.
    pub handover_bus: Option<BusId>,
    pub stop: Option<i64>,
    pub spot: Option<usize>,
    /// What state 1 walks to (+0x5bd) and whether it is a point of the bus (`bus`) or of
    /// the world; the heading to turn to there (+0x5d4).
    pub target: DVec3,
    pub target_bus: bool,
    pub target_yaw: f64,
    /// The path point walked from / to (+0x5e4) and the one at the end (+0x5e0).
    pub pt: Option<usize>,
    pub pt_target: Option<usize>,
    /// Stop 0.7 m short of the target (+0x5ec).
    pub short: bool,
    /// Walking to a door from outside (+0x5d0): keep 0.5 m off the bus side
    /// (`clamp_x`, +0x5cc) unless the door is open (+0x5d1) and they are level with it;
    /// a door on the left (+0x5d2).
    pub clamp: bool,
    pub clamp_open: bool,
    pub clamp_left: bool,
    pub clamp_x: f64,
    pub journey: Journey,
    /// The place reserved in the bus (+0x610).
    pub seat: Option<usize>,
    /// Kept from boarders until its occupant has stood up from it.
    pub vacating: Option<usize>,
    /// +0x61c, the ticket (1-based, +0x61d) and its price (+0x620), what was paid (+0x624),
    /// the change was wrong (+0x628), the cash desk was free (+0x629), the ticket sale
    /// step (+0x6c6).
    pub ticket: TicketAction,
    pub ticket_id: u8,
    pub price: f32,
    pub paid: f32,
    pub bad_change: bool,
    pub fare_phase: FarePhase,
    /// The entry or exit asked for (+0x640).
    pub door: Option<usize>,
    /// `HeightOfSeat` (+0x648) and `PAX_State` (+0x64c: 0 stand, 1 walk, 2 sit).
    pub seat_h: f32,
    pub posture: Posture,
    /// A countdown in seconds (+0x650) and one in metres walked (+0x654).
    pub timer: f32,
    pub dist_timer: f32,
    /// The angles ease (+0x660); talking to the driver (+0x662); the right hand reaches
    /// (+0x665) for `reach_at` (+0x67c, bus frame); the head turns to the driver (+0x666).
    pub smooth: bool,
    pub talking: bool,
    pub reach: bool,
    pub look_driver: bool,
    pub reach_at: Vec3,
    /// The room height of the link walked (+0x668), its step sounds (+0x694), the link
    /// (+0x698).
    pub room: f32,
    pub step_pack: Option<usize>,
    pub link: Option<usize>,
    /// Speed wanted (+0x6a0), speed (+0x6a4), walking pace (+0x6ac).
    pub speed_des: f32,
    pub speed: f32,
    pub walk_speed: f32,
    /// Somebody in the way (+0x6c7: 1 behind, 2 in front facing them, 3 in front going
    /// the same way or busy) and on which sides there is room (+0x6c8, +0x6c9).
    pub obstruction: Obstruction,
    /// Seconds held up by somebody in front inside a bus, and seconds left passing them
    /// (see `pax_move`).
    pub jam: f32,
    pub squeeze: f32,
    pub free_r: bool,
    pub free_l: bool,
    /// How badly the ride has gone (+0x62c, 0..1; see `ride_comfort`), the complaint said
    /// so far (+0x630: 1 TooBad_A, 2 TooBad_B, 3 TooBad_C - and off at the next stop) and
    /// where each one comes (+0x634, +0x638, +0x63c; drawn once, 0 not yet).
    pub discomfort: f32,
    pub complaint: Complaint,
    pub bad_at: [f32; 3],
    /// Distance moved this frame (+0x644, `LastMovedDist`).
    pub moved: f32,
    /// Seconds waiting at a closed reachable exit before trying an open alternative.
    pub door_wait: f32,
}

impl Pax {
    pub(in crate::humans) fn new(walk_speed: f32) -> Pax {
        Pax {
            accepted_transfer: None,
            seat_approach: None,
            seat_floor: None,
            doorway: None,
            task: Task::Nothing,
            movement: Movement::Standing,
            inside: None,
            pos: DVec3::ZERO,
            yaw: 0.0,
            bus: None,
            handover_bus: None,
            stop: None,
            spot: None,
            target: DVec3::ZERO,
            target_bus: false,
            target_yaw: 0.0,
            pt: None,
            pt_target: None,
            short: false,
            clamp: false,
            clamp_open: false,
            clamp_left: false,
            clamp_x: -1e9,
            journey: Journey {
                dest: None,
                line: None,
                allowed_termini: None,
                alt: None,
                alt_seen: false,
                alt_m: 0.0,
                ride_km: 1.0,
                km_start: 0.0,
            },
            seat: None,
            vacating: None,
            ticket: TicketAction::None,
            ticket_id: 0,
            price: 0.0,
            paid: 0.0,
            bad_change: false,
            fare_phase: FarePhase::None,
            door: None,
            seat_h: 0.0,
            posture: Posture::Standing,
            timer: 0.0,
            dist_timer: 0.0,
            smooth: false,
            talking: false,
            reach: false,
            look_driver: false,
            reach_at: Vec3::ZERO,
            room: OUTSIDE_ROOM,
            step_pack: None,
            link: None,
            speed_des: 0.0,
            speed: 0.0,
            walk_speed,
            obstruction: Obstruction::Clear,
            jam: 0.0,
            squeeze: 0.0,
            free_r: true,
            free_l: true,
            discomfort: 0.0,
            complaint: Complaint::None,
            bad_at: [0.0; 3],
            moved: 0.0,
            door_wait: 0.0,
        }
    }
}

/// An exit crossing stays in the cabin frame until the feet reach its exterior point.
#[derive(Debug, Clone, Copy)]
pub(in crate::humans) struct Doorway {
    pub target: Vec3,
    pub stop: Option<i64>,
}

/// Walk to the place in front of a seat, turn round, and sit down.
#[derive(Debug, Clone, Copy)]
pub(in crate::humans) struct SeatApproach {
    pub target: DVec3,
    pub yaw: f64,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::humans) struct SeatFloor {
    pub center: glam::DVec2,
    pub radius: f64,
    pub z: f64,
    pub around: f64,
}

impl SeatFloor {
    pub(in crate::humans) fn at(&self, p: glam::DVec2) -> f64 {
        if (p - self.center).length() < self.radius {
            self.z
        } else {
            self.around
        }
    }
}

/// The room height outside a vehicle (+0x668 = 50).
pub(in crate::humans) const OUTSIDE_ROOM: f32 = 50.0;

/// OMSI compatibility values are converted only for scripts and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(in crate::humans) enum Movement {
    Standing = 0,
    ToTarget = 1,
    ShortOfTarget = 2,
    AtTarget = 3,
    AlongPath = 5,
    ShortOfPathEnd = 6,
    AtPathEnd = 7,
    Turning = 9,
}
impl std::fmt::Display for Movement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

/// OMSI compatibility values are converted only for scripts and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(in crate::humans) enum FarePhase {
    None = 0,
    Validating = 1,
    Validated = 2,
    RequestTicket = 3,
    Paying = 4,
    AwaitTicket = 5,
    TakingTicket = 6,
    AwaitChange = 7,
    TakingChange = 8,
}
impl std::fmt::Display for FarePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

/// OMSI compatibility values are converted only for scripts and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(in crate::humans) enum Obstruction {
    Clear = 0,
    Behind = 1,
    Facing = 2,
    Busy = 3,
}
impl std::fmt::Display for Obstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

/// OMSI compatibility values are converted only for scripts and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(in crate::humans) enum Complaint {
    None = 0,
    Mild = 1,
    Strong = 2,
    Leave = 3,
}
impl std::fmt::Display for Complaint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

/// OMSI compatibility values are converted only for scripts and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(in crate::humans) enum Posture {
    Standing = 0,
    Walking = 1,
    Sitting = 2,
}
impl std::fmt::Display for Posture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

/// The ticket a passenger has (+0x61c): nothing to do, a ticket to stamp, one to buy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(in crate::humans) enum TicketAction {
    None = 0,
    Stamp = 2,
    Buy = 3,
}

/// Journey selection belongs to the origin stop or LAN host; cabin motion only
/// updates progress. Local timetable indices never cross the network boundary.
#[derive(Debug, Clone)]
pub(in crate::humans) struct Journey {
    /// Destination (+0x5f4), the stop's line record it matched (+0x5f8), the stop the line
    /// leaves the known route at (+0x5fc) and whether it was passed (+0x600), the way on
    /// from there (+0x604, m), the distance to ride (+0x5f0, km) and the odometer at boarding
    /// (+0x60c, km).
    pub dest: Option<String>,
    pub line: Option<usize>,
    /// Semantic line permission from the host; independent of a client's timetable cache.
    pub allowed_termini: Option<HashSet<String>>,
    pub alt: Option<String>,
    pub alt_seen: bool,
    pub alt_m: f32,
    pub ride_km: f32,
    pub km_start: f64,
}
