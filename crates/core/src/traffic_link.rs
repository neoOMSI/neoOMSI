//! What the traffic is told about the player and the LAN buses.

use super::*;

/// Position, heading, half extents and speed of the player's vehicle for the AI's
/// obstacle checks.
pub(crate) fn player_outline(p: &Player) -> traffic::PlayerBox {
    vehicle_outline(&p.vehicle, p.vehicle.physics.speed)
}

/// `player_outline` of any vehicle moving at `speed` (m/s).
pub(crate) fn vehicle_outline(v: &::simulation::VehicleInstance, speed: f32) -> traffic::PlayerBox {
    let bb =
        v.ty.def
            .bounding_box
            .unwrap_or([2.5, 11.0, 3.0, 0.0, 0.0, 1.5]);
    let h = v.heading.to_radians();
    let centre = v.position
        + DVec3::new(
            (bb[3] as f64) * h.cos() + (bb[4] as f64) * h.sin(),
            -(bb[3] as f64) * h.sin() + (bb[4] as f64) * h.cos(),
            0.0,
        );
    (centre, v.heading, bb[1] * 0.5, bb[0] * 0.5, speed)
}

/// Everything of the player's besides the bus's own box that the traffic has to keep out
/// of: the rear sections of an articulated bus or a coupled trailer (the traffic saw only
/// the front section and drove into the back of a turning GN92), and the vehicles placed
/// by hand from the vehicle list, with their trailers - under ids of their own beside the
/// LAN players'.
pub(crate) fn own_outlines(
    player: Option<&Player>,
    placed: &[Player],
) -> Vec<(u32, traffic::PlayerBox)> {
    let mut out = Vec::new();
    let mut add = |v: &::simulation::VehicleInstance, base: u32, whole: bool| {
        let speed = v.physics.speed;
        if whole {
            out.push((base, vehicle_outline(v, speed)));
        }
        for (k, t) in v.trailers.iter().enumerate() {
            let Some(bb) = t.ty.def.bounding_box else {
                continue;
            };
            let h = t.heading.to_radians();
            let centre = t.position
                + DVec3::new(
                    (bb[3] as f64) * h.cos() + (bb[4] as f64) * h.sin(),
                    -(bb[3] as f64) * h.sin() + (bb[4] as f64) * h.cos(),
                    0.0,
                );
            out.push((
                base + 1 + k as u32,
                (centre, t.heading, bb[1] * 0.5, bb[0] * 0.5, speed),
            ));
        }
    };
    if let Some(p) = player {
        add(&p.vehicle, 0xFFFF_0000, false);
    }
    for (i, q) in placed.iter().enumerate() {
        add(&q.vehicle, 0xFFFE_0000 - (i as u32) * 16, true);
    }
    out
}

/// The LAN players' buses as obstacles for the AI traffic (their speed as last sent).
/// A remote's trailers share its id, so perception sees them as parts of the same vehicle
/// and the traffic cannot drive through the back of an articulated remote bus.
pub(crate) fn lan_outlines(game: &lan::LanGame) -> Vec<(u32, traffic::PlayerBox)> {
    let mut out = Vec::new();
    for (id, r) in game.remotes.iter() {
        let v = r.vehicle();
        let speed = r.last.speed_kmh / 3.6;
        out.push((*id, vehicle_outline(v, speed)));
        for t in &v.trailers {
            let Some(bb) = t.ty.def.bounding_box else {
                continue;
            };
            let h = t.heading.to_radians();
            let centre = t.position
                + DVec3::new(
                    (bb[3] as f64) * h.cos() + (bb[4] as f64) * h.sin(),
                    -(bb[3] as f64) * h.sin() + (bb[4] as f64) * h.cos(),
                    0.0,
                );
            out.push((*id, (centre, t.heading, bb[1] * 0.5, bb[0] * 0.5, speed)));
        }
    }
    // Deterministic order regardless of the remote map's hash order.
    out.sort_by_key(|(id, _)| *id);
    out
}

/// The LAN players whose state says they are on an emergency drive (`FLAG_EMERGENCY`).
pub(crate) fn lan_emergencies(game: &lan::LanGame) -> Vec<u32> {
    let mut out: Vec<u32> = game
        .remotes
        .iter()
        .filter(|(_, r)| r.last.flags & ::network::FLAG_EMERGENCY != 0)
        .map(|(id, _)| *id)
        .collect();
    out.sort_unstable();
    out
}

/// What the traffic needs to know every frame besides the time: where the player looks
/// from, the day of the week, who walks the footpaths, what hides what.
pub(crate) fn traffic_inputs(
    t: &mut traffic::Traffic,
    cam: Option<&Camera>,
    aspect: f64,
    fog: f64,
    clock: &::simulation::SimClock,
    humans: Option<&humans::Humans>,
    player: Option<&Player>,
    render: &::render::RenderOptions,
) {
    if let Some(c) = cam {
        t.set_viewer(Some(
            traffic::Viewer::new(c, aspect, fog)
                .with_culling(render.min_obj_size, render.max_obj_dist),
        ));
    }
    t.set_world_inputs(
        clock.weekday(),
        humans.map(|h| h.strollers()).unwrap_or_default(),
        humans.map(|h| h.on_foot()).unwrap_or_default(),
        // the player's obstacle boxes follow the streamed tiles; without a player the
        // world's own are asked
        player.and_then(|p| p.vehicle.collision.clone()),
    );
}

/// Posts (`[crashmode_pole]`) the vehicle knocked over this frame: laid on the ground from
/// their foot in the direction they were hit.
pub(crate) fn lay_down_poles(
    world: &World,
    renderer: &Renderer,
    scene: &mut Scene,
    vehicle: &mut ::simulation::VehicleInstance,
) {
    for (id, push) in std::mem::take(&mut vehicle.knocked_now) {
        if let Some(pos) = world.lay_down_pole(renderer, scene, id, push) {
            log::info!("knocked over post {id} at ({:.1}, {:.1})", pos.x, pos.y);
        }
    }
}
