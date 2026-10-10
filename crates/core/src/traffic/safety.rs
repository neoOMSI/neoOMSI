//! Full-body scenery validation at the domain/engine boundary (including mesh obstacles).
use super::*;
use ::traffic::perception::SweepSample;

#[derive(Clone)]
pub(super) struct SweepTrailer {
    pub position: DVec3,
    pub heading: f64,
    pub back: DVec2,
    pub front: DVec2,
    pub length: f64,
    pub bbox: [f32; 6],
    pub lift: f64,
    pub max_angle: Option<f64>,
}

fn rotated(v: DVec2, heading: f64) -> DVec2 {
    let (s, c) = heading.to_radians().sin_cos();
    DVec2::new(v.x * c + v.y * s, -v.x * s + v.y * c)
}

/// Sweep articulated sections about their own axles. Extending the tractor's box
/// to the back of a bendy bus rotates the entire rear at once and invents a wide
/// tail swing into parked cars that the real articulation never makes.
pub(super) fn articulated_clear(
    collision: &::simulation::collision::CollisionWorld,
    occupancy: &Occupancy,
    samples: &[SweepSample],
    actor: &ManeuverActor,
    contact: Option<&dyn ::simulation::rigid::Ground>,
    primary_rear: f32,
    trailers: &[SweepTrailer],
    tractor: &AiBody,
) -> bool {
    let Some(first) = samples.first() else { return false; };
    let last = samples.last().unwrap();
    let way = |d: f32| {
        let i = samples.partition_point(|s| s.d < d);
        if i == 0 { return first.p + first.dir.extend(0.0) * (d - first.d) as f64; }
        if i >= samples.len() { return last.p + last.dir.extend(0.0) * (d - last.d) as f64; }
        let (a, b) = (samples[i - 1], samples[i]);
        a.p.lerp(b.p, ((d - a.d) / (b.d - a.d).max(0.001)) as f64)
    };
    let poses = tractor.predict_poses(&way, actor.speed, actor.accel,
        (actor.max_speed_kmh / 3.6).min(14.0), last.d);
    let mut primary_actor = actor.clone();
    primary_actor.rear = primary_rear;
    let realized_samples: Vec<_> = poses.iter().map(|&(d, p, h)| SweepSample {
        p, d, dir: rotated(DVec2::Y, h),
    }).collect();
    if !scenery_clear(collision, &realized_samples, &primary_actor, contact) { return false; }
    let mut parts = trailers.to_vec();
    let mut pivots: Vec<_> = parts.iter().map(|t| t.position.truncate()
        + rotated(t.front - DVec2::Y * t.length, t.heading)).collect();
    for (_, p, h) in poses {
        let dir = rotated(DVec2::Y, h);
        let mut lead = p.truncate();
        let mut heading = h;
        let road = contact.and_then(|g| g.road_height(p.x, p.y, p.z, 1.5)).unwrap_or(p.z);
        let mut primary = Obb::vehicle(lead, h, actor.front as f64, primary_rear as f64, actor.half_width as f64);
        primary.z0 = road + 0.15;
        primary.z1 = road + actor.height as f64;
        let footprint = BodyFootprint::new(actor.id, primary.center, dir,
            primary.half.y, primary.half.x, primary.z0, primary.z1, actor.speed);
        let mut blocked = false;
        occupancy.near(primary.center, primary.radius(), |other| {
            blocked |= other.owner != actor.id && other.overlaps(&footprint, 0.0);
        });
        if blocked { return false; }
        for (part, pivot) in parts.iter_mut().zip(&mut pivots) {
            let coupling = lead + rotated(part.back, heading);
            let aim = (coupling - *pivot).normalize_or_zero();
            let mut h = aim.x.atan2(aim.y).to_degrees();
            if let Some(max) = part.max_angle {
                let delta = (heading - h + 540.0).rem_euclid(360.0) - 180.0;
                h = heading - delta.clamp(-max, max);
            }
            *pivot = coupling - rotated(DVec2::Y * part.length, h);
            let origin = coupling - rotated(part.front, h);
            let road = contact.and_then(|g| g.road_height(origin.x, origin.y, p.z, 1.5)).unwrap_or(p.z);
            let body = Obb::from_box(part.bbox, origin.extend(road + part.lift), h);
            if collision.hit(&body).is_some() { return false; }
            let fwd = rotated(DVec2::Y, h);
            let footprint = BodyFootprint::new(actor.id, body.center, fwd,
                body.half.y, body.half.x, body.z0, body.z1, actor.speed);
            let mut blocked = false;
            occupancy.near(body.center, body.radius(), |other| {
                blocked |= other.owner != actor.id && other.overlaps(&footprint, 0.0);
            });
            if blocked { return false; }
            lead = origin;
            heading = h;
        }
    }
    true
}

/// Side envelope of the realized rigid body relative to its lane. Long overhangs
/// matter while leaving a bend: treating a bus as an aligned width-only strip
/// misses the front corner beside a parked car.
pub(super) fn side_extent(front: f32, rear: f32, half_width: f32, yaw_deg: f32, side: f32) -> f32 {
    let (s, c) = yaw_deg.to_radians().sin_cos();
    half_width * c.abs() + (front * s * side).max(-rear * s * side).max(0.0)
}

/// A blocked side corner may be cleared inside the current lane. This is a
/// steering request, never a pose correction; realization still rejects contact.
pub(super) fn corner_swerve(
    collision: &::simulation::collision::CollisionWorld,
    occupancy: &Occupancy,
    owner: VehicleId,
    net: &Network,
    state: &AiState,
    body: &AiBody,
    vehicle: &VehicleInstance,
    caps: &VehicleCapabilities,
    ahead: Option<f32>,
) -> Option<f32> {
    let mut probe = body.clone();
    probe.step(0.1, 0.5, &|d| state.way_point(net, d),
        vehicle.ground.as_ref().map(|g| g.as_ref() as &dyn Fn(f64, f64) -> Option<f64>),
        vehicle.contact.as_deref());
    let bbox = road_body_box(vehicle, &probe, caps);
    let (bbox, obstacle) = match collision.hit(&bbox) {
        Some(obstacle) => (bbox, obstacle),
        None => {
            // Not touching yet: the vehicle stopped short of what its realization saw
            // coming (`scenery_ahead` metres on). Find that contact along the way, so the
            // swerve is asked for before the bumper is against it.
            let way = |d: f32| state.way_point(net, d);
            let mut found = None;
            let mut last = -1.0f32;
            for (d, p, heading) in body.predict_poses(&way, 1.0, 0.3, 1.0, ahead? + 0.6) {
                if d - last < 0.3 { continue; }
                last = d;
                probe.position = p;
                probe.position.z = vehicle.contact.as_deref()
                    .and_then(|g| g.road_height(p.x, p.y, p.z, 1.5)).unwrap_or(p.z);
                probe.heading = heading;
                let at = road_body_box(vehicle, &probe, caps);
                if let Some(obstacle) = collision.hit(&at) {
                    found = Some((at, obstacle));
                    break;
                }
            }
            found?
        }
    };
    let touch = bbox.contact(&obstacle)?;
    let lane = net.lanes.get(state.lane)?;
    let (s, _) = lane.nearest_point(body.position)?;
    let (p, heading) = lane.at(s);
    let h = (heading as f64).to_radians();
    let right = DVec2::new(h.cos(), -h.sin());
    let side = touch.normal.dot(right);
    if side.abs() < 0.5 { return None; } // a wall ahead is a stop, not a side swerve
    let lateral = (body.position - p).truncate().dot(right) as f32;
    let mut room = (lane.width * 0.5 - caps.half_width - 0.05).max(0.0);
    // Long bodies need swing room on junction aprons, where car centre paths
    // alone understate the available turning space. Verify that space physically.
    if lane.turn != 0 || !net.crossings[state.lane].is_empty() { room = room.max(1.0); }
    // A wide vehicle (a bus in a narrow lane) has almost no room left inside its lane, so a
    // swerve was never found and it stood against the parked car for good. Being against
    // something already is reason enough to lean over the lane edge; every candidate path is
    // checked against the real collision boxes and other vehicles below.
    room = room.max(0.6);
    let sign = side.signum() as f32;
    'candidates: for step in [0.35f32, 0.6, 0.2] {
        let target = (state.lateral_target + sign * step).clamp(-room, room);
        if (target - lateral).abs() <= 0.05 { continue; }
        let mut path = state.clone();
        path.lateral_target = target;
        path.lateral_ramp = (lateral, target, state.odometer, 2.0);
        let poses = body.predict_poses(&|d| path.way_point(net, d), state.speed, 0.8, 3.0, caps.rear + 5.0);
        for (_, p, heading) in poses {
            probe.position = p;
            probe.position.z = vehicle.contact.as_deref()
                .and_then(|g| g.road_height(p.x, p.y, p.z, 1.5)).unwrap_or(p.z);
            probe.heading = heading;
            let bbox = road_body_box(vehicle, &probe, caps);
            if collision.hit(&bbox).is_some() { continue 'candidates; }
            let footprint = BodyFootprint::new(owner, bbox.center, rotated(DVec2::Y, heading),
                bbox.half.y, bbox.half.x, bbox.z0, bbox.z1, state.speed);
            let mut blocked = false;
            occupancy.near(bbox.center, bbox.radius(), |other| {
                blocked |= other.owner != owner && other.overlaps(&footprint, 0.0);
            });
            if blocked { continue 'candidates; }
        }
        return Some(target);
    }
    None
}

pub(super) fn vehicle_height(vehicle: &VehicleInstance) -> f32 {
    vehicle
        .ty
        .def
        .bounding_box
        .map(|b| vehicle.ai_rest_offset().0 + b[5] + b[2] * 0.5)
        .or_else(|| vehicle.ty.model_box().map(|(_, hi)| vehicle.ai_rest_offset().0 + hi.z))
        .filter(|h| h.is_finite() && *h > 0.3)
        .unwrap_or(3.5)
}

/// Realization checks the model's physical box; passing's extra margin is not solid body.
pub(super) fn road_body_box(vehicle: &VehicleInstance, body: &AiBody, caps: &VehicleCapabilities) -> Obb {
    let position = body.position + DVec3::new(0.0, 0.0, vehicle.ai_rest_offset().0 as f64);
    if let Some(bb) = vehicle.ty.def.bounding_box {
        return Obb::from_box(bb, position, body.heading);
    }
    let mut bbox = Obb::vehicle(position.truncate(), body.heading, caps.front as f64, caps.rear as f64, caps.half_width as f64);
    let (bottom, top) = vehicle.ty.model_box().map(|(lo, hi)| (lo.z as f64, hi.z as f64))
        .unwrap_or((0.15, 3.5));
    bbox.z0 = position.z + bottom;
    bbox.z1 = position.z + top;
    bbox
}

pub(super) fn external_actor(
    net: &Network,
    id: VehicleId,
    p: PlayerBox,
    active: bool,
) -> Option<(JunctionActor, Vec<(usize, f32)>)> {
    let (lane, s, _) = net.lane_along(p.0, p.1, LaneKind::Street, 4.0, 45.0)?;
    let mut actor = JunctionActor::new(id, lane, s);
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
    contact: Option<&dyn ::simulation::rigid::Ground>,
) -> bool {
    samples.iter().all(|s| {
        let heading = s.dir.x.atan2(s.dir.y).to_degrees();
        let mut body = Obb::vehicle(
            s.p.truncate(),
            heading,
            actor.front as f64,
            actor.rear as f64,
            actor.half_width as f64,
        );
        // The path's height is only a reference: Berlin's authored asphalt can sit
        // above it. Use the same nearest-road-level query as the tyres, so asphalt
        // cannot become a wall and a bridge above this road cannot lift the sweep.
        let road_z = contact.and_then(|g| g.road_height(s.p.x, s.p.y, s.p.z, 1.5))
            .unwrap_or(s.p.z);
        body.z0 = road_z + 0.15;
        body.z1 = road_z + actor.height as f64;
        let hit = collision.hit(&body);
        if hit.is_some() && ::legacy_config::env::var_os("OMSI_DEBUG_AI_SWEEP").is_some() {
            static REJECTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            if REJECTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 500 == 0 {
                log::info!("AI maneuver scenery rejection: vehicle {}, lane {}, rear {:.2}, sample {:.1} at {:?}, road {:.3}, body {:?}, obstacle {:?}",
                    actor.id, actor.lane, actor.rear, s.d, s.p, road_z, body, hit);
            }
        }
        hit.is_none()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::simulation::collision::CollisionWorld;
    use ::simulation::rigid::Ground;
    use glam::Vec3;
    #[test]
    fn an_articulated_lane_change_does_not_rotate_the_whole_rear_into_a_parked_car() {
        let mut actor = ManeuverActor::new(VehicleId(93), 0, 0.0);
        actor.front = 5.68;
        actor.rear = 11.2;
        actor.half_width = 1.24;
        let path = |d: f32| {
            let u = (d / 12.0).clamp(0.0, 1.0);
            DVec3::new((-3.5 * u * u * (3.0 - 2.0 * u)) as f64, d as f64, 0.0)
        };
        let samples: Vec<_> = (0..=12).map(|d| SweepSample {
            p: path(d as f32), d: d as f32,
            dir: (path(d as f32 + 0.1) - path(d as f32)).truncate().normalize_or_zero(),
        }).collect();
        let trailer = SweepTrailer { position: DVec3::new(0.0, -7.6, 0.0), heading: 0.0,
            back: DVec2::new(0.0, -4.0), front: DVec2::new(0.0, 3.6), length: 4.8,
            bbox: [2.48, 7.2, 3.0, 0.0, 0.0, 1.65], lift: 0.0, max_angle: Some(47.0) };
        let mut world = CollisionWorld::default();
        let parked = |x| Obb::from_box([1.79, 4.2, 1.6, 0.0, 0.0, 0.8], DVec3::new(x, -9.0, 0.0), 0.0);
        world.add(parked(2.37));
        let occupancy = Occupancy::default();
        use ::legacy_vehicle::vehicle::Axle;
        let def = ::legacy_vehicle::Vehicle { mass: 12.0, rot_pnt_long: -3.0, inv_min_turn_radius: 0.1,
            axles: vec![Axle { long: 2.9, ..Default::default() }, Axle { long: -3.0, ..Default::default() }],
            ..Default::default() };
        let mut body = AiBody::new(&def, MotionKind::Road);
        body.place(&|d| DVec3::new(0.0, d as f64, 0.0), None, None, 0.0);
        assert!(!scenery_clear(&world, &samples, &actor, None), "reproduces the false rigid tail swing");
        assert!(articulated_clear(&world, &occupancy, &samples, &actor, None, 3.88, &[trailer.clone()], &body));
        world.add(parked(2.05));
        assert!(!articulated_clear(&world, &occupancy, &samples, &actor, None, 3.88, &[trailer], &body),
            "an actual rear-section obstruction still blocks the change");
    }
    #[test]
    fn a_bus_leaving_a_bend_reserves_room_for_its_front_corner() {
        let left = side_extent(5.68, 3.88, 1.24, -8.5, -1.0);
        let right = side_extent(5.68, 3.88, 1.24, -8.5, 1.0);
        assert!(left > 2.05 && left < 2.1);
        assert!(right > 1.78 && right < 1.82);
        assert_eq!(side_extent(5.68, 3.88, 1.24, 0.0, -1.0), 1.24);
    }
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
        assert!(scenery_clear(&world, &samples, &actor, None));
        // Centerline does not hit this post, but the vehicle side does.
        world.add(Obb::from_box(
            [0.3, 0.3, 2.0, 0.0, 0.0, 1.0],
            DVec3::new(4.2, 9.0, 0.0),
            0.0,
        ));
        assert!(!scenery_clear(&world, &samples, &actor, None));
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
        assert!(scenery_clear(&world, &samples, &actor, None));
    }

    #[test]
    fn existing_side_clearance_does_not_become_a_phantom_scenery_overlap() {
        let mut actor = ManeuverActor::new(VehicleId(93), 0, 0.0);
        actor.front = 5.68;
        actor.rear = 12.03;
        actor.half_width = 1.24;
        let samples = [SweepSample { p: DVec3::ZERO, d: 0.0, dir: DVec2::Y }];
        let mut world = CollisionWorld::default();
        world.add(Obb::from_box([1.79, 4.2, 1.6, 0.0, 0.0, 0.8],
            DVec3::new(2.37, -9.0, 0.0), 0.0));
        assert!(scenery_clear(&world, &samples, &actor, None),
            "the bus already fits beside this parked car with 23 cm clearance");
    }

    #[test]
    fn a_road_above_its_path_does_not_veto_a_bus_lane_change() {
        use ::simulation::collision::{MeshObstacle, MeshShape};
        use ::simulation::rigid::GroundProbe;
        let mut actor = ManeuverActor::new(VehicleId(93), 0, 0.0);
        actor.front = 5.68;
        actor.rear = 3.88;
        actor.half_width = 1.24;
        let samples = [SweepSample { p: DVec3::new(-1.5, 8.0, 0.03), d: 8.0, dir: DVec2::Y }];
        let mut world = CollisionWorld::default();
        let corners = [DVec3::new(-20.0, -20.0, 0.45), DVec3::new(20.0, -20.0, 0.45),
            DVec3::new(20.0, 20.0, 0.45), DVec3::new(-20.0, 20.0, 0.45)];
        let mesh = MeshShape::from_triangles([[corners[0], corners[1], corners[2]],
            [corners[0], corners[2], corners[3]]].into_iter(), 0.3);
        world.add_mesh(MeshObstacle::new(Arc::new(mesh), DVec3::ZERO, 0.0, 1));
        let ground = |_: f64, _: f64, _: f64| GroundProbe {
            below: Some(0.45), above: Some(5.0), normal: None,
        };
        assert!(!scenery_clear(&world, &samples, &actor, None), "reproduces the old asphalt hit");
        assert!(scenery_clear(&world, &samples, &actor, Some(&ground)));
        world.add(Obb::from_box([0.3, 0.3, 2.0, 0.0, 0.0, 1.0],
            DVec3::new(-0.2, 9.0, 0.45), 0.0));
        assert!(!scenery_clear(&world, &samples, &actor, Some(&ground)), "real posts remain obstacles");
    }
}
