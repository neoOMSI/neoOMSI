//! The roofs, canopies, halls and bridge decks that keep the rain and snow off (#137).
//!
//! A cell's roof is its highest face - a road or `[surface]`, or a solid triangle of a
//! sheltering object - when something lies at least [`CLEARANCE`] below it.

use crate::scene::World;
use glam::{DVec2, DVec3};
use std::time::{Duration, Instant};

/// The size of a cell (m).
pub const CELL: f64 = 1.0;
const SIDE: usize = ::render::COVER_SIDE as usize;
/// How far under the highest face something has to lie for that face to be a roof (m).
const CLEARANCE: f64 = 1.5;
/// A cell without a roof (far below any ground, also for the shader).
const NO_ROOF: f32 = -1.0e9;

/// The middle of the cell `c` along one axis.
fn centre(c: i64) -> f64 {
    (c as f64 + 0.5) * CELL
}

/// The height of the roof over (x, y), if there is one (see the module).
pub fn roof_at(world: &World, x: f64, y: f64) -> Option<f64> {
    let at = DVec2::new(x, y);
    let blockers = world.camera_blockers(at, at);
    roof_among(world, &blockers, at)
}

/// [`roof_at`] with the placed objects that may reach over the point already gathered.
fn roof_among(
    world: &World,
    blockers: &[(
        std::sync::Arc<crate::scene::ObjectType>,
        crate::camera_arm::Blocker,
    )],
    at: DVec2,
) -> Option<f64> {
    let mut top = world.camera_ground(at.x, at.y, 1.0e5);
    for (ot, b) in blockers {
        let Some(shape) = ot.shelter_shape() else {
            continue;
        };
        if (b.pos.truncate() - at).length() > b.radius {
            continue;
        }
        // straight down from over the object (a metre spare for tilt and scale)
        let reach = shape.lo.length().max(shape.hi.length()) as f64 + 1.0;
        let from = DVec3::new(at.x, at.y, b.pos.z + reach);
        if let Some(t) =
            crate::camera_arm::ray_object(ot, shape, b.pos, &b.xf, from, DVec3::NEG_Z, 2.0 * reach)
        {
            let z = from.z - t;
            if top.is_none_or(|h| z > h) {
                top = Some(z);
            }
        }
    }
    let top = top?;
    world
        .camera_ground(at.x, at.y, top - CLEARANCE)
        .map(|_| top)
}

/// The roofs of the cells around the camera.
pub struct Shelter {
    /// Per slot (column `x mod SIDE`, row `y mod SIDE` of the world cell): the height of
    /// the roof, or [`NO_ROOF`].
    roofs: Vec<f32>,
    /// The world cell each slot holds and the tiles' generation it was found for.
    held: Vec<Option<((i64, i64), u64)>>,
    /// The world cell at the square's south-west corner.
    corner: (i64, i64),
    changed: bool,
}

impl Default for Shelter {
    fn default() -> Self {
        Self::new()
    }
}

impl Shelter {
    pub fn new() -> Shelter {
        Shelter {
            roofs: vec![NO_ROOF; SIDE * SIDE],
            held: vec![None; SIDE * SIDE],
            corner: (0, 0),
            changed: false,
        }
    }

    fn slot(c: (i64, i64)) -> usize {
        let s = SIDE as i64;
        (c.1.rem_euclid(s) * s + c.0.rem_euclid(s)) as usize
    }

    fn cell_of(x: f64, y: f64) -> (i64, i64) {
        ((x / CELL).floor() as i64, (y / CELL).floor() as i64)
    }

    /// Move the square to `eye` and find the roofs of new or outdated cells, the rows
    /// nearest the eye first, for at most `budget`.
    pub fn update(&mut self, world: &World, eye: DVec3, budget: Option<Duration>) {
        let generation = world
            .tiles_generation
            .load(std::sync::atomic::Ordering::Relaxed);
        self.fill(eye, generation, budget, |cy, cxs| {
            let y = centre(cy);
            let blockers = world.camera_blockers(
                DVec2::new(cxs[0] as f64 * CELL, y),
                DVec2::new((cxs[cxs.len() - 1] + 1) as f64 * CELL, y),
            );
            cxs.iter()
                .map(|&cx| roof_among(world, &blockers, DVec2::new(centre(cx), y)))
                .collect()
        });
    }

    /// [`Shelter::update`] with the roofs from `roofs_of(row, columns)`.
    fn fill(
        &mut self,
        eye: DVec3,
        generation: u64,
        budget: Option<Duration>,
        mut roofs_of: impl FnMut(i64, &[i64]) -> Vec<Option<f64>>,
    ) {
        let start = Instant::now();
        let c = Self::cell_of(eye.x, eye.y);
        let half = (SIDE / 2) as i64;
        self.corner = (c.0 - half, c.1 - half);
        for d in 0..SIDE {
            let row = if d % 2 == 0 {
                SIDE / 2 + d / 2
            } else {
                SIDE / 2 - 1 - d / 2
            };
            let cy = self.corner.1 + row as i64;
            let stale: Vec<i64> = (0..SIDE as i64)
                .map(|i| self.corner.0 + i)
                .filter(|&cx| self.held[Self::slot((cx, cy))] != Some(((cx, cy), generation)))
                .collect();
            if stale.is_empty() {
                continue;
            }
            for (cx, roof) in stale.iter().zip(roofs_of(cy, &stale)) {
                let i = Self::slot((*cx, cy));
                self.roofs[i] = roof.map_or(NO_ROOF, |h| h as f32);
                self.held[i] = Some(((*cx, cy), generation));
            }
            self.changed = true;
            if budget.is_some_and(|b| start.elapsed() > b) {
                break;
            }
        }
    }

    /// The roof over (x, y) as last found, if the cell is in the square.
    pub fn roof(&self, x: f64, y: f64) -> Option<f64> {
        let c = Self::cell_of(x, y);
        let i = Self::slot(c);
        match self.held[i] {
            Some((held, _)) if held == c && self.roofs[i] > NO_ROOF => Some(self.roofs[i] as f64),
            _ => None,
        }
    }

    /// Whether the cell of (x, y) is in the square and has been looked at.
    fn knows(&self, x: f64, y: f64) -> bool {
        let c = Self::cell_of(x, y);
        self.held[Self::slot(c)].is_some_and(|(held, _)| held == c)
    }

    /// Whether a roof is over the point.
    pub fn covers(&self, p: DVec3) -> bool {
        self.roof(p.x, p.y).is_some_and(|r| p.z < r)
    }

    /// As [`Shelter::covers`], asking the world outside the square.
    pub fn covers_anywhere(&self, world: &World, p: DVec3) -> bool {
        if self.knows(p.x, p.y) {
            self.covers(p)
        } else {
            roof_at(world, p.x, p.y).is_some_and(|r| p.z < r)
        }
    }

    /// Hand the square to the renderer (the cells only when they changed).
    pub fn upload(&mut self, renderer: &::render::Renderer) {
        let s = SIDE as i64;
        renderer.set_cover_map(
            self.corner.0 as f64 * CELL,
            self.corner.1 as f64 * CELL,
            CELL as f32,
            (
                self.corner.0.rem_euclid(s) as u32,
                self.corner.1.rem_euclid(s) as u32,
            ),
            self.changed.then_some(self.roofs.as_slice()),
        );
        self.changed = false;
    }
}

/// A square around the origin under one roof at `height` everywhere.
#[cfg(test)]
pub(crate) fn roofed(height: f64) -> Shelter {
    let mut s = Shelter::new();
    s.fill(DVec3::ZERO, 0, None, |_, cxs| vec![Some(height); cxs.len()]);
    s
}

/// The front of a vehicle's roof (`[boundingbox]`): sheltered, its windscreen stays dry.
pub fn windscreen_top(v: &::simulation::VehicleInstance) -> Option<DVec3> {
    let bb = v.ty.def.bounding_box?;
    let (s, c) = v.heading.to_radians().sin_cos();
    let fwd = (bb[4] + bb[1] * 0.5 - 0.3) as f64;
    let side = bb[3] as f64;
    let up = (bb[5] + bb[2] * 0.5) as f64;
    Some(v.position + DVec3::new(c * side + s * fwd, -s * side + c * fwd, up))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bridge 8 m up over the band 10 <= y < 20, the sky everywhere else.
    fn bridge(cy: i64, cxs: &[i64]) -> Vec<Option<f64>> {
        let y = centre(cy);
        cxs.iter()
            .map(|_| (10.0..20.0).contains(&y).then_some(8.0))
            .collect()
    }

    #[test]
    fn a_roof_keeps_the_weather_off_what_is_under_it_only() {
        let mut s = Shelter::new();
        s.fill(DVec3::ZERO, 0, None, bridge);
        // under the deck, on its top, beside it
        assert!(s.covers(DVec3::new(0.0, 15.0, 3.0)));
        assert!(s.covers(DVec3::new(-40.0, 10.2, 7.9)));
        assert!(!s.covers(DVec3::new(0.0, 15.0, 8.0)));
        assert!(!s.covers(DVec3::new(0.0, 15.0, 12.0)));
        assert!(!s.covers(DVec3::new(0.0, 9.9, 3.0)));
        assert!(!s.covers(DVec3::new(0.0, 20.1, 3.0)));
        // beyond the square nothing is known
        assert!(!s.covers(DVec3::new(500.0, 15.0, 3.0)));
    }

    #[test]
    fn the_square_follows_the_eye_and_finds_only_what_it_lacks() {
        let mut s = Shelter::new();
        let mut asked = 0;
        s.fill(DVec3::ZERO, 0, None, |cy, cxs| {
            asked += cxs.len();
            bridge(cy, cxs)
        });
        assert_eq!(asked, SIDE * SIDE);
        // 30 m on: 30 new columns in every row, the rest is kept
        let mut asked = 0;
        s.fill(DVec3::new(30.0, 0.0, 0.0), 0, None, |cy, cxs| {
            asked += cxs.len();
            bridge(cy, cxs)
        });
        assert_eq!(asked, 30 * SIDE);
        assert!(s.covers(DVec3::new(100.0, 15.0, 3.0)));
        assert!(!s.covers(DVec3::new(-80.0, 15.0, 3.0)));
        // tiles came or went: everything is looked at again, and meanwhile kept
        let mut asked = 0;
        s.fill(
            DVec3::new(30.0, 0.0, 0.0),
            1,
            Some(Duration::ZERO),
            |cy, cxs| {
                asked += cxs.len();
                // the eye's row first
                assert_eq!(cy, 0);
                bridge(cy, cxs)
            },
        );
        assert_eq!(asked, SIDE);
        assert!(s.covers(DVec3::new(30.0, 15.0, 3.0)));
    }

    #[test]
    fn the_renderer_gets_the_cells_only_when_they_changed() {
        let mut s = Shelter::new();
        assert!(!s.changed);
        s.fill(DVec3::ZERO, 0, None, bridge);
        assert!(s.changed);
        s.changed = false;
        s.fill(DVec3::ZERO, 0, None, bridge);
        assert!(!s.changed);
    }
}
