//! Full-body scenery validation at the domain/engine boundary (including mesh obstacles).
use super::*;
use ::traffic::perception::SweepSample;

pub(super) fn vehicle_height(vehicle: &VehicleInstance) -> f32 {
    vehicle
        .ty
        .def
        .bounding_box
        .map(|b| b[2])
        .or_else(|| vehicle.ty.model_box().map(|(lo, hi)| hi.z - lo.z))
        .filter(|h| h.is_finite() && *h > 0.3)
        .unwrap_or(3.5)
}

pub(super) fn external_actor(
    net: &Network,
    p: PlayerBox,
    active: bool,
) -> Option<(JunctionActor, Vec<(usize, f32)>)> {
    let (lane, s, _) = net.lane_along(p.0, p.1, LaneKind::Street, 4.0, 45.0)?;
    let mut actor = JunctionActor::new(VehicleId(u64::MAX), lane, s);
    actor.front = p.2;
    actor.rear = p.2;
    actor.length = p.2 * 2.0;
    actor.speed = p.4.max(0.0);
    actor.emergency = active;
    actor.priority = active;
    let mut way = vec![(lane, -s)];
    let mut current = lane;
    let mut d = net.lanes[lane].length() - s;
    for _ in 0..16 {
        if d > 160.0 {
            break;
        }
        let h = net.lanes[current].end_heading();
        let next = net.lanes[current]
            .next
            .iter()
            .copied()
            .filter(|n| net.lanes[*n].kind == LaneKind::Street)
            .min_by(|a, b| {
                let turn = |n: usize| {
                    ((net.lanes[n].start_heading() - h + 540.0).rem_euclid(360.0) - 180.0).abs()
                };
                turn(*a).total_cmp(&turn(*b)).then(a.cmp(b))
            });
        let Some(n) = next else {
            break;
        };
        if way.iter().any(|w| w.0 == n) {
            break;
        }
        way.push((n, d));
        d += net.lanes[n].length();
        current = n;
    }
    Some((actor, way))
}

pub(super) fn scenery_clear(
    collision: &::simulation::collision::CollisionWorld,
    samples: &[SweepSample],
    actor: &ManeuverActor,
) -> bool {
    samples.iter().all(|s| {
        let heading = s.dir.x.atan2(s.dir.y).to_degrees();
        let mut body = Obb::vehicle(
            s.p.truncate(),
            heading,
            actor.front as f64,
            actor.rear as f64,
            actor.half_width as f64 + ::traffic::PULL_OUT_CLEARANCE,
        );
        // Exclude road faces below the tyres; preserve walls/posts at vehicle height.
        body.z0 = s.p.z + 0.15;
        body.z1 = s.p.z + actor.height as f64;
        collision.hit(&body).is_none()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::simulation::collision::CollisionWorld;
    use ::simulation::rigid::Ground;
    use glam::Vec3;
    #[test]
    fn authored_drive_faces_win_over_buried_terrain_and_keep_bridge_levels_separate() {
        let mut surface = ::geometry::TileSurface::new(300);
        for z in [0.9, 5.0] {
            surface.drive.push([
                Vec3::new(0.0, 0.0, z),
                Vec3::new(20.0, 0.0, z),
                Vec3::new(0.0, 20.0, z),
            ]);
        }
        surface.drive.build(300.0);
        let ground = crate::scene::DriveGround {
            terrains: Default::default(),
            surfaces: Default::default(),
        };
        ground.surfaces.write().insert((0, 0), Arc::new(surface));
        for (reference, expected) in [(0.0, 0.9), (5.0, 5.0), (1.1, 0.9)] {
            assert!(
                (ground.road_height(2.0, 2.0, reference, 1.5).unwrap() - expected).abs() < 1e-5
            );
        }
    }
    #[test]
    fn issue_126_wall_beside_bus_blocks_the_whole_passing_body() {
        let actor = ManeuverActor::new(VehicleId(1), 0, 0.0);
        let samples = [SweepSample {
            p: DVec3::new(3.0, 8.0, 0.0),
            d: 8.0,
            dir: DVec2::Y,
        }];
        let mut world = CollisionWorld::default();
        assert!(scenery_clear(&world, &samples, &actor));
        // Centerline does not hit this post, but the vehicle side does.
        world.add(Obb::from_box(
            [0.3, 0.3, 2.0, 0.0, 0.0, 1.0],
            DVec3::new(4.2, 9.0, 0.0),
            0.0,
        ));
        assert!(!scenery_clear(&world, &samples, &actor));
    }
    #[test]
    fn elevated_object_does_not_block_the_road_below() {
        let actor = ManeuverActor::new(VehicleId(1), 0, 0.0);
        let samples = [SweepSample {
            p: DVec3::ZERO,
            d: 0.0,
            dir: DVec2::Y,
        }];
        let mut world = CollisionWorld::default();
        world.add(Obb::from_box(
            [3.0, 10.0, 1.0, 0.0, 0.0, 0.0],
            DVec3::new(0.0, 0.0, 6.0),
            0.0,
        ));
        assert!(scenery_clear(&world, &samples, &actor));
    }
}
