use super::*;

#[derive(Debug, Clone)]
pub(super) enum State {
    /// A pedestrian strolling the pavements (Omsi.exe's task 8, `WalkStreet`).
    Strolling(PedWalk),
    /// Moved by somebody else: an avatar, or one of a LAN host's people.
    Idle,
    /// Task 8 without a path (+0x2f0 = -1): somebody who got off where no pavement is
    /// stands where they are until the player is gone.
    Standing,
    /// A passenger (see `passengers`).
    Pax(Box<Pax>),
}

impl State {
    pub(super) fn name(&self) -> &'static str {
        match self {
            State::Strolling(_) => "WalkStreet",
            State::Idle => "Idle",
            State::Standing => "WalkStreet",
            State::Pax(p) => p.task.name(),
        }
    }
    pub(super) fn bus(&self) -> Option<BusId> {
        match self {
            State::Pax(p) => p.inside.or(p.bus),
            _ => None,
        }
    }
}

/// Where a person is: on the ground, or inside a bus at a point of its frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Place {
    Ground,
    Bus(BusId, Vec3),
}

/// A point of a floor frame in the model frame of somebody standing at `origin` facing
/// `heading` (degrees).
pub(super) fn model_point(origin: DVec3, heading: f64, q: DVec3) -> Vec3 {
    let d = q - origin;
    let h = heading.to_radians();
    let (s, c) = (h.sin(), h.cos());
    Vec3::new(
        (d.x * c - d.y * s) as f32,
        (d.x * s + d.y * c) as f32,
        d.z as f32,
    )
}

pub struct Person {
    pub(super) render: PersonRender,
    pub(super) id: u32,
    pub(super) ty: Arc<HumanType>,
    /// Clothing variant (`HumanType::variant_texture`).
    pub(super) variant: usize,
    /// Authoritative for pedestrians and mirrors. For local passengers this is the
    /// world-space projection of Pax's authoritative pose, refreshed after each tick.
    /// Cabin movement and task transitions write Pax, never this projection.
    pub(super) position: DVec3,
    pub(super) heading: f64,
    /// Heading in the bus frame while inside one.
    pub(super) lheading: f64,
    pub(super) place: Place,
    /// Velocity in the plane the person walks in (ground or bus floor).
    pub(super) vel: DVec2,
    pub(super) pace: f64,
    pub(super) activity: Activity,
    /// The animation: Omsi.exe's walk phase and joint angles (sub_626ae8).
    pub(super) anim: OmsiAnim,
    /// Procedural inverse kinematics pose (stride-driven feet, sitting easing, looking).
    pub pose: omsi_sim::human::Pose,
    pub(super) state: State,
    /// Seconds in the current state.
    pub(super) t_state: f32,
    /// Interior light of the bus the person is in (0 outside).
    pub(super) interior: f32,
    /// The tilt of the floor the person stands on (a bus pitching under the brakes and
    /// leaning in a bend), without its heading: riders are drawn with it. Upright on the
    /// ground; drawn upright in a tilted bus, their feet sank through the floor on one side.
    pub(super) tilt: Mat4,
    /// Age in years: the `.hum`'s `[age]`, else 40 as in OMSI. The
    /// ticket pack's tickets have age ranges (the reduced fare is for 6..13).
    pub(super) age: f32,
    /// Seconds without getting nearer the goal while wanting to move; seconds left
    /// passing through others.
    pub(super) stuck: f32,
    pub(super) ghost: f32,
    /// Seconds a standing vehicle has stood in the way (see the crowd step).
    pub(super) car_wait: f32,
    /// Seconds left going round something in the way off the pavement's line (a lamp post
    /// on the path): the corridor does not pull them back into it meanwhile.
    pub(super) detour: f32,
    /// Which way round (+1 anticlockwise, -1 clockwise) while `detour` lasts: round a corner
    /// the sides' own choices flipped each other and people shuffled at a post.
    pub(super) detour_side: f64,
    /// Why the person is standing, for `OMSI_DEBUG_PAX`.
    pub(super) why: &'static str,
    /// A scripted test person (`OMSI_PAX_GALLERY`).
    pub(super) puppet: Option<Puppet>,
    /// LAN play: one of the host's people, drawn where the host says (`mirror_set`).
    pub(super) remote: bool,
}

impl Person {
    /// Keep the local passenger root and its world-space projection on one floor.
    pub(super) fn set_ground_height(&mut self, height: f64, procedural: bool) {
        if let State::Pax(p) = &mut self.state {
            if !self.remote {
                if p.inside.is_some() || (p.posture == Posture::Sitting && !procedural) {
                    return;
                }
                p.pos.z = height;
            }
        }
        self.position.z = height;
        self.pose.set_ground_height(height);
    }
    pub fn state_name(&self) -> String {
        format!("#{} {} ({})", self.id, self.state.name(), self.why)
    }
    pub fn position(&self) -> DVec3 {
        self.position
    }
    pub(super) fn inside(&self, bus: BusId) -> bool {
        matches!(self.place, Place::Bus(b, _) if b == bus)
    }
    pub(super) fn label(&self) -> String {
        format!(
            "#{} {}",
            self.id,
            self.ty
                .def
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        )
    }
}
