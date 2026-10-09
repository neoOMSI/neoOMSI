use super::*;
use ::network::passenger::PassengerGrant;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    root: PathBuf,
    world: World,
    human: Arc<HumanType>,
}

fn noop_renderer() -> Renderer {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::NOOP;
    descriptor.backend_options.noop = wgpu::NoopBackendOptions::enabled();
    let instance = wgpu::Instance::new(descriptor);
    pollster::block_on(Renderer::new_with(
        &instance,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        ::render::RenderOptions::default(),
    ))
    .unwrap()
}

#[test]
fn omsi_comparison_mode_preserves_the_original_rendered_bones() {
    let f = Fixture::new();
    let mut person = f.person(5, State::Standing, false);
    for _ in 0..120 {
        person.finish_animation(
            false,
            &::simulation::human_omsi::AnimInput {
                kind: 1,
                speed: 1.3,
                moved: 1.3 / 60.0,
                dt_ms: 1000.0 / 60.0,
                room_height: 50.0,
                ..Default::default()
            },
        );
        assert_eq!(
            person.render.active_bones.unwrap(),
            ::simulation::human::slots_from_omsi(&person.anim.bones(&person.ty.omsi))
        );
    }
}

#[test]
fn switching_lod_uses_the_current_pose_and_recycles_contact_shadows() {
    use ::simulation::human::{HumanMesh, Influence, PoseInput, hand_slot};
    let f = Fixture::new();
    let renderer = noop_renderer();
    let mut scene = renderer.new_scene();
    let mut ty = HumanType::load(&f.root.join("person.hum")).unwrap();
    let data = ::geometry::MeshData {
        positions: vec![
            Vec3::new(0.7, 0.0, 1.4),
            Vec3::new(0.8, 0.0, 1.4),
            Vec3::new(0.7, 0.0, 1.5),
        ],
        normals: vec![Vec3::Y; 3],
        uvs: vec![glam::Vec2::ZERO; 3],
        indices: vec![0, 1, 2],
        ranges: vec![(0, 3, 0)],
        ..Default::default()
    };
    let mesh = || HumanMesh {
        data: data.clone(),
        materials: vec![::legacy_o3d::Material::default()],
        bones: vec![(6, vec![(0, 1.0), (1, 1.0), (2, 1.0)])],
        skin: vec![
            Influence {
                n: 1,
                slot: [hand_slot(1) as u8, 0, 0, 0],
                weight: [1.0, 0.0, 0.0, 0.0]
            };
            3
        ],
        alpha: vec![0],
    };
    ty.meshes = vec![mesh()];
    ty.lower = vec![(1, mesh())];
    ty.levels = vec![(0.25, f32::MAX), (0.0, 0.25)];
    let mut h = Humans::new(&f.root);
    set_population(&mut h, [Arc::new(ty)]);
    h.ik = true;
    let i = h
        .spawn(
            &f.world,
            &renderer,
            &mut scene,
            DVec3::ZERO,
            0.0,
            State::Standing,
        )
        .unwrap();
    let p = &mut h.people[i];
    for _ in 0..60 {
        p.pose.advance(
            &p.ty.rig,
            &PoseInput {
                activity: Activity::Sit,
                seat: Some(Vec3::new(0.0, -0.15, 0.5)),
                ..Default::default()
            },
            0.1,
        );
    }
    assert!(p.pose.sit_amount() > 0.98);
    p.finish_animation(true, &Default::default());
    for (distance, expected) in [(2.0, 0), (30.0, 1), (2.0, 0)] {
        h.sync(&renderer, &mut scene, DVec3::new(0.0, -distance, 1.0));
        let p = &h.people[i];
        assert_eq!(p.render.level, expected);
        for (k, (_, instance)) in p.render.meshes.iter().enumerate() {
            assert_eq!(
                scene.instances[*instance].visible,
                p.ty.mesh_at(k).0 == expected
            );
            assert_ne!(p.render.skins[k].0, p.ty.mesh_at(k).1.data.positions);
            assert!(p.render.skins[k].0.iter().all(|v| v.is_finite()));
        }
        assert!(p.render.blob.is_none());
    }
    h.people[i].pose = ::simulation::human::Pose::new(1);
    let rig = h.people[i].ty.rig.clone();
    h.people[i].pose.advance(&rig, &PoseInput::default(), 0.1);
    h.people[i].finish_animation(true, &Default::default());
    h.sync(&renderer, &mut scene, DVec3::new(0.0, -2.0, 1.0));
    let blob = h.people[i].render.blob.unwrap();
    let retired = h.people.remove(i);
    h.retire(&retired);
    let i = h
        .spawn(
            &f.world,
            &renderer,
            &mut scene,
            DVec3::ZERO,
            0.0,
            State::Standing,
        )
        .unwrap();
    h.sync(&renderer, &mut scene, DVec3::new(0.0, -2.0, 1.0));
    assert_eq!(h.people[i].render.blob, Some(blob));
    assert!(scene.instances[blob].visible);
}

#[test]
fn natural_ground_avoidance_keeps_roots_coherent_and_leaves_waiters_fixed() {
    let f = Fixture::new();
    for natural in [false, true] {
        let mut h = Humans::new(&f.root);
        h.set_ik(false);
        h.set_natural(natural);
        let mut moving = Pax::new(1.0);
        moving.task = Task::ToBus;
        moving.target = DVec3::new(0.0, 10.0, 0.0);
        moving.speed = 1.0;
        moving.pos = DVec3::new(0.0, 0.1, 0.0);
        let mut person = f.person(1, State::Pax(Box::new(moving)), false);
        person.position = DVec3::new(0.0, 0.1, 0.0);
        person.vel = DVec2::Y;
        h.people.push(person);
        let mut waiter = f.person(2, State::Standing, false);
        waiter.position = DVec3::new(0.1, 0.6, 0.0);
        h.people.push(waiter);
        let before = h.people[0].position;
        let fixed = h.people[1].position;
        h.pax_room(0.1, &[DVec3::ZERO, fixed], &[], &HashMap::new());
        assert_eq!(h.people[0].position, h.pax(0).unwrap().pos);
        assert_eq!(h.people[1].position, fixed);
        assert!((h.people[0].position - before).length() <= 0.080001);
        if natural {
            assert_ne!(h.people[0].position, before);
        } else {
            assert_eq!(h.people[0].position, before);
        }
    }
}

#[test]
fn overlapping_stops_cannot_spawn_people_in_the_same_physical_waiting_place() {
    let f = Fixture::new();
    let renderer = noop_renderer();
    let mut scene = renderer.new_scene();
    for height in [0.0, 0.5] {
        let mut h = Humans::new(&f.root);
        set_population(&mut h, [f.human.clone()]);
        h.ik = true;
        for id in [1, 2] {
            let mut s = stop(DVec3::new(2.0, 2.0, 0.0), 0.0, "Shared platform");
            s.spots[0].pos = DVec3::new(2.0, 2.0, height as f64);
            s.spots[0].height = height;
            h.stops.insert(id, s);
        }
        assert!(
            h.spawn_waiting(&f.world, &renderer, &mut scene, 1)
                .is_some()
        );
        assert!(
            h.spawn_waiting(&f.world, &renderer, &mut scene, 2)
                .is_none(),
            "independent stop reservations cannot claim the same physical place"
        );
        assert_eq!(h.people.len(), 1);
    }
}

#[test]
fn a_waiting_place_excludes_its_own_actor_and_separates_different_floors() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut s = stop(DVec3::new(2.0, 2.0, 0.0), 0.0, "Origin");
    s.spots[0].pos = DVec3::new(2.0, 2.0, 0.0);
    h.stops.insert(1, s);
    let mut p = f.person(7, State::Standing, false);
    p.position = DVec3::new(2.0, 2.0, 0.0);
    h.people.push(p);
    assert_eq!(h.take_spot(1, None), None);
    assert_eq!(h.take_spot(1, Some(7)), Some(0));
    h.free_spot(1, 0);
    h.people[0].position.z = 3.0;
    assert_eq!(h.take_spot(1, None), Some(0));
}

#[test]
fn host_id_collisions_cannot_consume_a_local_fare_owner_or_alias_an_avatar() {
    let f = Fixture::new();
    let renderer = noop_renderer();
    let mut scene = renderer.new_scene();
    let mut h = Humans::new(&f.root);
    set_population(&mut h, [f.human.clone()]);
    let host_id = (1 << 30) + 7;
    let mut p = Pax::new(1.1);
    p.task = Task::InBusToPlace;
    p.bus = Some(BusId::Player);
    h.people
        .push(f.person(host_id, State::Pax(Box::new(p)), false));
    h.desk.desk_busy = Some(host_id);
    h.request = Some(("Single".into(), 2.5));
    h.next_id = host_id + 3;
    h.stops.insert(1, stop(DVec3::ZERO, 0.0, "Origin"));
    let grant = PassengerGrant {
        id: host_id,
        transfer: 1,
        stop: 1,
        spot: 0,
        destination: Some("Host journey".into()),
        alternative: None,
        alternative_m: 0.0,
        ride_km: 5.0,
        line_destination: None,
        allowed_termini: None,
    };
    assert!(
        !h.grant(&grant),
        "a retained local person is not a receipt for a host transfer"
    );
    let pose = MirrorPose {
        pos: DVec3::ZERO,
        heading: 0.0,
        vel: DVec2::ZERO,
        activity: Activity::Stand,
        aboard: None,
        waiting: Some((1, 0)),
    };
    assert!(h.mirror_add(&f.world, &renderer, &mut scene, host_id, 0, &pose));
    let owner = h.desk.desk_busy.unwrap();
    assert_ne!(owner, host_id);
    assert!(h.people.iter().any(|p| p.id == owner && !p.remote));
    assert!(h.grant(&grant));
    assert!(h.grant(&grant));
    assert_eq!(h.people.iter().filter(|p| p.id == host_id).count(), 1);
    assert_eq!(h.desk.desk_busy, Some(owner));

    let avatar_id = host_id + 1;
    let mut avatar = f.person(avatar_id, State::Idle, false);
    avatar.puppet = Some(Puppet {
        mode: PuppetMode::Avatar,
    });
    h.people.push(avatar);
    h.avatars.avatars.insert(99, avatar_id);
    h.avatars.avatar_hidden.insert(avatar_id, true);
    assert!(h.mirror_add(&f.world, &renderer, &mut scene, avatar_id, 0, &pose));
    let avatar = h.avatars.avatars[&99];
    assert_ne!(avatar, avatar_id);
    assert_eq!(h.avatars.avatar_hidden.get(&avatar), Some(&true));
    let next = h.next_id;
    h.people.push(f.person(next, State::Idle, true));
    let i = h
        .spawn(
            &f.world,
            &renderer,
            &mut scene,
            DVec3::ZERO,
            0.0,
            State::Standing,
        )
        .unwrap();
    assert_ne!(
        h.people[i].id, next,
        "new local people must not reuse a received host id"
    );
    let ids = h.people.iter().map(|p| p.id).collect::<HashSet<_>>();
    assert_eq!(ids.len(), h.people.len());
}

#[test]
fn procedural_ai_riders_follow_cabin_paths_before_the_seat_transition() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut b = bus(cabin());
    b.id = BusId::Ai(42);
    let ix = [(b.id, 0)].into_iter().collect();
    let mut p = Pax::new(1.1);
    p.task = Task::WalkingToBus;
    p.bus = Some(b.id);
    p.seat = Some(1);
    p.door = Some(0);
    p.pos = DVec3::ZERO;
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
    h.set_task(
        0,
        Task::InBusToPlace,
        std::slice::from_ref(&b),
        &ix,
        &f.world,
    );
    assert_eq!(h.pax(0).unwrap().task, Task::InBusToPlace);
    assert!(h.pax(0).unwrap().seat_approach.is_none());
    let mut previous = h.pax(0).unwrap().pos;
    for _ in 0..150 {
        h.pax_frame(
            0.05,
            &f.world,
            None,
            std::slice::from_ref(&b),
            &ix,
            &HashMap::new(),
            None,
            &mut |_, _, _, _| panic!("no sale"),
            &mut false,
            &mut vec![],
        );
        h.animate(0.05, &f.world, std::slice::from_ref(&b), &ix);
        let p = h.pax(0).unwrap();
        assert!((p.pos - previous).length() <= 0.06);
        previous = p.pos;
    }
    assert_eq!(h.pax(0).unwrap().task, Task::SittingInBus);
    assert!(h.people[0].pose.sit_amount() > 0.98);
}

#[test]
fn a_seated_lan_mirror_keeps_its_seat_pose_without_running_a_local_journey() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut b = bus(cabin());
    b.id = BusId::Ai(remote_bus_id(2));
    let local = Vec3::new(0.0, 2.0 + f.human.rig.seat_front(), 0.0);
    h.people.push(f.person(7, State::Idle, true));
    h.mirror_set(
        7,
        &MirrorPose {
            pos: b.world(local),
            heading: 0.0,
            vel: DVec2::ZERO,
            activity: Activity::Sit,
            aboard: Some((remote_bus_id(2), local, 0.0, Some(1))),
            waiting: None,
        },
    );
    let ix = [(b.id, 0)].into_iter().collect();
    for _ in 0..60 {
        h.animate(0.05, &f.world, std::slice::from_ref(&b), &ix);
    }
    assert_eq!(h.people[0].activity, Activity::Sit);
    assert!(h.people[0].pose.sit_amount() > 0.98);
    assert_eq!(
        h.people[0].pose.floor_pose(),
        (b.id.space(), local.as_dvec3(), 0.0)
    );
    assert!(matches!(h.people[0].state, State::Idle));
    assert!(h.people[0].remote && h.pax(0).is_none());
}

#[test]
fn a_seated_waiting_lan_mirror_uses_the_maps_bench_point() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    h.ik = true;
    let mut s = stop(DVec3::new(2.0, 2.0, 0.0), 0.0, "Origin");
    s.spots[0].pos = DVec3::new(2.0, 2.0, 0.5);
    s.spots[0].height = 0.5;
    s.spots[0].face = 75.0;
    let root = s.spots[0].foot_root(f.human.rig.seat_front(), 0.0);
    h.stops.insert(1, s);
    h.people.push(f.person(7, State::Idle, true));
    h.mirror_set(
        7,
        &MirrorPose {
            pos: root,
            heading: 75.0,
            vel: DVec2::ZERO,
            activity: Activity::Sit,
            aboard: None,
            waiting: Some((1, 0)),
        },
    );
    h.animate(0.05, &f.world, &[], &HashMap::new());
    let p = &mut h.people[0];
    assert_eq!(p.activity, Activity::Sit);
    assert_eq!(p.pose.sit_amount(), 1.0);
    let hip = root.as_vec3()
        + Mat4::from_rotation_z(-75.0_f32.to_radians())
            .transform_vector3(p.render.active_bones.unwrap()[8].transform_point3(p.ty.rig.pelvis));
    assert!((hip.truncate() - Vec3::new(2.0, 2.0, 0.5).truncate()).length() < 0.05);
    assert!(p.remote && matches!(p.state, State::Idle));
}

#[test]
fn walking_lan_mirrors_animate_from_cabin_motion_without_including_bus_world_motion() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut b = bus(cabin());
    b.id = BusId::Ai(remote_bus_id(2));
    h.people.push(f.person(7, State::Idle, true));
    let ix = [(b.id, 0)].into_iter().collect();
    let mut landings = 0;
    for frame in 0..90 {
        b.pos.y += 0.5;
        let local = Vec3::Y * (frame as f32 * 0.05 * 1.1);
        h.mirror_set(
            7,
            &MirrorPose {
                pos: b.world(local),
                heading: 0.0,
                vel: DVec2::ZERO,
                activity: Activity::Walk,
                aboard: Some((remote_bus_id(2), local, 0.0, None)),
                waiting: None,
            },
        );
        h.animate(0.05, &f.world, std::slice::from_ref(&b), &ix);
        landings += usize::from(h.people[0].pose.landed());
        assert_eq!(h.people[0].pose.floor_pose().1, local.as_dvec3());
        assert!(
            h.people[0]
                .render
                .active_bones
                .unwrap()
                .iter()
                .all(|b| b.is_finite())
        );
    }
    assert!(
        (3..=14).contains(&landings),
        "local gait landed {landings} times"
    );
}

#[test]
fn opposite_aisle_and_stair_walkers_recover_without_teleport_or_persistent_overlap() {
    let f = Fixture::new();
    for stairs in [false, true] {
        let mut h = Humans::new(&f.root);
        let mut c = cabin();
        if stairs {
            let cabin = Arc::get_mut(&mut c).unwrap();
            let points = (0..5)
                .map(|i| Vec3::new(0.0, i as f32, i as f32 * 0.5))
                .collect();
            cabin.graph = PathGraph::new(points, &cabin.links);
            cabin.routes = pax::build_routes(&cabin.graph, &cabin.links);
        }
        let mut b = bus(c);
        let ix = [(b.id, 0)].into_iter().collect();
        for (id, y, from, to, yaw) in [(0, 1.3, 2, 3, 0.0), (1, 1.7, 1, 0, std::f64::consts::PI)] {
            let mut p = Pax::new(1.1);
            p.inside = Some(b.id);
            p.bus = Some(b.id);
            p.yaw = yaw;
            p.pos = DVec3::new(0.0, y, if stairs { y * 0.5 } else { 0.0 });
            p.pt = Some(from);
            p.pt_target = Some(to);
            p.movement = Movement::AlongPath;
            p.posture = Posture::Walking;
            let position = p.pos;
            let mut person = f.person(id, State::Pax(Box::new(p)), false);
            person.position = position;
            person.heading = yaw.to_degrees();
            h.people.push(person);
        }
        let mut previous = h.people.iter().map(|p| p.position).collect::<Vec<_>>();
        let mut smooth = [::simulation::human::Pose::new(0), ::simulation::human::Pose::new(1)];
        let mut overlap = 0;
        let mut maximum_overlap = 0;
        for frame in 0..600 {
            b.accel.y = if frame < 20 { -6.0 } else { 0.0 };
            h.pax_frame(
                0.05,
                &f.world,
                None,
                std::slice::from_ref(&b),
                &ix,
                &HashMap::new(),
                None,
                &mut |_, _, _, _| panic!("no payment"),
                &mut false,
                &mut vec![],
            );
            h.animate(0.05, &f.world, std::slice::from_ref(&b), &ix);
            for (i, p) in h.people.iter().enumerate() {
                assert!((p.position - previous[i]).length() <= 0.08);
                let (frame, origin, heading) = p.pose.floor_pose();
                let floor = |_: DVec2| Some(origin.z);
                smooth[i].advance(
                    &p.ty.rig,
                    &::simulation::human::PoseInput {
                        frame,
                        origin,
                        heading,
                        activity: p.activity,
                        velocity: p.vel,
                        floor: Some(&floor),
                        ..Default::default()
                    },
                    0.05,
                );
                let baseline = smooth[i].bones(&p.ty.rig);
                let root_delta = p.render.active_bones.unwrap()[8]
                    .transform_point3(p.ty.rig.pelvis)
                    - baseline.bones[8].transform_point3(p.ty.rig.pelvis);
                assert!(
                    root_delta.length() < 0.3,
                    "braking displaced the body {root_delta:?}"
                );
                previous[i] = p.position;
            }
            if (h.people[0].position - h.people[1].position).length() < 0.25 {
                overlap += 1;
            } else {
                overlap = 0;
            }
            maximum_overlap = maximum_overlap.max(overlap);
        }
        assert!(
            h.people[0].position.y > 2.8 && h.people[1].position.y < 0.2,
            "walkers remained jammed: {:?} {:?}",
            h.people[0].position,
            h.people[1].position
        );
        assert!(
            maximum_overlap < 15,
            "overlap lasted {maximum_overlap} frames"
        );
    }
}

fn departure_crowd_frame(h: &mut Humans, world: &World, net: &Network, buses: &[BusNow], dt: f32) {
    let ix = buses.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
    let regs = buses
        .iter()
        .map(|b| {
            (
                b.id,
                pax::BusAtStops {
                    next: Some(1),
                    at: Some(1),
                    ..Default::default()
                },
            )
        })
        .collect();
    h.time += dt as f64;
    let mut removed = vec![];
    h.pax_frame(
        dt,
        world,
        Some(net),
        buses,
        &ix,
        &regs,
        None,
        &mut |_, _, _, _| panic!("no sale"),
        &mut false,
        &mut removed,
    );
    let who = h
        .people
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            (p.place == Place::Ground && !matches!(p.state, State::Pax(_)))
                || matches!(&p.state,State::Pax(x) if x.doorway.is_some())
        })
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    let wants = who
        .iter()
        .map(|&i| h.decide(i, dt, world, Some(net), None, &[], &mut removed))
        .collect::<Vec<_>>();
    let mut walkers = who
        .iter()
        .zip(&wants)
        .map(|(&i, w)| Walker {
            pos: h.people[i].position.truncate(),
            vel: if matches!(h.people[i].state, State::Pax(_)) {
                DVec2::ZERO
            } else {
                h.people[i].vel
            },
            radius: BODY_OUTSIDE,
            want: w.vel,
            give: w.give,
            space: 0,
            fixed: matches!(h.people[i].state, State::Pax(_)),
            ghost: h.people[i].ghost > 0.0,
            corridor: w.corridor,
        })
        .collect::<Vec<_>>();
    let blocks = buses.iter().flat_map(BusNow::blocks).collect::<Vec<_>>();
    crowd::step(&mut walkers, &blocks, &CrowdParams::default(), dt as f64);
    for ((&i, w), want) in who.iter().zip(&walkers).zip(&wants) {
        if !matches!(h.people[i].state, State::Pax(_)) {
            h.apply(i, w, want, dt, world, Some(net), buses, &ix);
        }
    }
    h.animate(dt, world, buses, &ix);
    assert!(removed.is_empty());
}

#[test]
fn consecutive_alighters_queue_at_the_door_and_disperse_around_existing_pedestrians() {
    let f = Fixture::new();
    let point = |x, y| DVec3::new(x, y, 0.0);
    let cases = [
        vec![(point(3.5, 5.0), point(3.5, 35.0))],
        vec![(point(3.5, 5.0), point(3.5, 7.0))],
        vec![
            (point(3.5, 5.0), point(3.5, 8.0)),
            (point(3.5, 8.0), point(6.5, 8.0)),
            (point(6.5, 8.0), point(6.5, 5.0)),
            (point(6.5, 5.0), point(3.5, 5.0)),
        ],
        vec![
            (point(3.5, 5.0), point(3.5, 7.0)),
            (point(3.5, 7.0), point(6.5, 7.0)),
            (point(3.5, 7.0), point(3.5, 10.0)),
        ],
        vec![],
    ];
    f.world
        .terrains
        .write()
        .insert((0, 0), Arc::new(::map::Terrain::flat()));
    for edges in cases {
        let mut h = Humans::new(&f.root);
        let mut c = cabin();
        Arc::get_mut(&mut c).unwrap().exits[1].outside = Vec3::new(1.5, 3.0, 0.0);
        let mut b = bus(c);
        b.pos = DVec3::new(2.0, 2.0, 0.0);
        let net = Network {
            lanes: edges
                .into_iter()
                .map(|(a, b)| {
                    ::traffic::LaneBuilder::polyline(vec![a, b], LaneKind::Sidewalk, 2.0)
                })
                .collect(),
            ..Default::default()
        };
        h.walking.ped = Some(PedNet::build(&net));
        let mut s = stop(DVec3::new(3.5, 5.0, 0.0), 0.0, "Origin");
        s.lane = Some((0, 0.0));
        h.stops.insert(1, s);
        for id in 0..6 {
            let mut p = Pax::new(1.1);
            p.task = Task::InBusToExit;
            p.inside = Some(b.id);
            p.bus = Some(b.id);
            p.pos = DVec3::Y * (0.4 + id as f64 * 0.4);
            p.pt = Some(3);
            p.pt_target = Some(3);
            p.door = Some(1);
            p.movement = Movement::AlongPath;
            p.posture = Posture::Walking;
            let position = b.world(p.pos.as_vec3());
            let mut person = f.person(id, State::Pax(Box::new(p)), false);
            person.position = position;
            h.people.push(person);
        }
        for id in 100..103 {
            let mut p = f.person(id, State::Standing, false);
            p.position = DVec3::new(3.5, 5.8 + (id - 100) as f64 * 0.6, 0.0);
            h.people.push(p);
        }
        let mut previous = h.people.iter().map(|p| p.position).collect::<Vec<_>>();
        for _ in 0..600 {
            departure_crowd_frame(&mut h, &f.world, &net, std::slice::from_ref(&b), 0.05);
            assert!(
                h.people
                    .iter()
                    .filter(|p| matches!(&p.state, State::Pax(x) if x.doorway.is_some()))
                    .count()
                    <= 1
            );
            for (i, p) in h.people.iter().enumerate() {
                assert!(
                    (p.position - previous[i]).length() <= 0.2,
                    "person {} jumped {:?} -> {:?}",
                    p.id,
                    previous[i],
                    p.position
                );
                previous[i] = p.position;
                if let State::Pax(x) = &p.state {
                    assert_eq!(
                        x.squeeze, 0.0,
                        "a same-direction exit queue must not squeeze through its leader"
                    );
                }
            }
        }
        assert!(
            h.people[..6]
                .iter()
                .all(|p| !matches!(p.state, State::Pax(_)))
        );
        assert!(
            h.people[..6]
                .iter()
                .all(|p| (p.position - DVec3::new(3.5, 5.0, 0.0)).length() > 2.0
                    || matches!(&p.state, State::Standing)
                    || matches!(&p.state,State::Strolling(walk) if walk.leg>=walk.legs.len()))
        );
        for a in 0..6 {
            for b in a + 1..6 {
                assert!((h.people[a].position - h.people[b].position).length() > 0.3);
            }
        }
    }
}

#[test]
fn a_spawned_waiting_root_and_pose_are_on_the_final_floor_before_rendering() {
    let f = Fixture::new();
    f.world
        .terrains
        .write()
        .insert((0, 0), Arc::new(::map::Terrain::flat()));
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::NOOP;
    descriptor.backend_options.noop = wgpu::NoopBackendOptions::enabled();
    let instance = wgpu::Instance::new(descriptor);
    let renderer = pollster::block_on(Renderer::new_with(
        &instance,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        ::render::RenderOptions::default(),
    ))
    .unwrap();
    let mut scene = renderer.new_scene();
    for (height, stature, heading) in [
        (0.0, 1.77, 0.0),
        (0.5, 1.3, 0.0),
        (0.5, 1.77, 75.0),
        (0.75, 2.1, -90.0),
    ] {
        let mut h = Humans::new(&f.root);
        set_population(&mut h, [f.scaled_human(stature)]);
        h.ik = true;
        let mut s = stop(DVec3::new(2.0, 2.0, 0.0), 0.0, "Origin");
        s.spots[0].pos = DVec3::new(2.0, 2.0, if height == 0.0 { -0.25 } else { height as f64 });
        s.spots[0].height = height;
        s.spots[0].face = heading;
        h.stops.insert(1, s);
        let i = h.spawn_waiting(&f.world, &renderer, &mut scene, 1).unwrap();
        let p = h.pax(i).unwrap().clone();
        assert_eq!(p.pos, h.people[i].position);
        assert_eq!(p.pos.z, 0.0);
        assert_eq!(
            h.people[i].pose.floor_pose(),
            (0, p.pos, p.yaw.to_degrees())
        );
        if height != 0.0 {
            assert_eq!(p.posture, Posture::Sitting);
            assert_eq!(h.people[i].pose.sit_amount(), 1.0);
            let person = &mut h.people[i];
            let bones = person.pose.bones(&person.ty.rig);
            assert!(bones.ok);
            let hip = p.pos.as_vec3()
                + Mat4::from_rotation_z(-person.heading.to_radians() as f32)
                    .transform_vector3(bones.bones[8].transform_point3(person.ty.rig.pelvis));
            let seat = h.stops[&1].spots[0].pos.as_vec3();
            assert!(
                (hip.truncate() - seat.truncate()).length() < 0.05,
                "hip {hip:?}, seat {seat:?}"
            );
            assert!((hip.z - seat.z - person.ty.rig.seat_lift).abs() < 0.05);
        }
        h.sync(&renderer, &mut scene, DVec3::ZERO);
        assert_eq!(h.people[i].render.posed_at.0, p.pos);
        let mut raised = ::map::Terrain::flat();
        raised.heights.fill(0.25);
        f.world.terrains.write().insert((0, 0), Arc::new(raised));
        h.populate(&f.world, &renderer, &mut scene, DVec3::ZERO);
        assert_eq!(h.pax(i).unwrap().pos.z, 0.25);
        assert_eq!(h.people[i].pose.floor_pose().1.z, 0.25);
        if height != 0.0 {
            let person = &mut h.people[i];
            let bones = person.pose.bones(&person.ty.rig);
            let hip = person.position.as_vec3()
                + Mat4::from_rotation_z(-person.heading.to_radians() as f32)
                    .transform_vector3(bones.bones[8].transform_point3(person.ty.rig.pelvis));
            assert!(
                (hip.z - height - person.ty.rig.seat_lift).abs() < 0.05,
                "streaming moved the bench hip: {hip:?}"
            );
        }
        h.pax_frame(
            0.05,
            &f.world,
            None,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            None,
            &mut |_, _, _, _| panic!("no sale"),
            &mut false,
            &mut vec![],
        );
        assert_eq!(h.people[i].position.z, 0.25);
        f.world
            .terrains
            .write()
            .insert((0, 0), Arc::new(::map::Terrain::flat()));
    }
}

#[test]
fn post_alight_walks_do_not_reverse_forever_in_dead_ends_cycles_or_junctions() {
    let f = Fixture::new();
    let point = |x, y| DVec3::new(x, y, 0.0);
    let cases = [
        vec![(point(2.0, 2.0), point(2.0, 4.0))],
        vec![
            (point(2.0, 2.0), point(2.0, 5.0)),
            (point(2.0, 5.0), point(5.0, 5.0)),
            (point(5.0, 5.0), point(5.0, 2.0)),
            (point(5.0, 2.0), point(2.0, 2.0)),
        ],
        vec![
            (point(2.0, 2.0), point(2.0, 5.0)),
            (point(2.0, 5.0), point(5.0, 5.0)),
            (point(2.0, 5.0), point(2.0, 9.0)),
        ],
    ];
    for edges in cases {
        let net = Network {
            lanes: edges
                .into_iter()
                .map(|(a, b)| {
                    ::traffic::LaneBuilder::polyline(vec![a, b], LaneKind::Sidewalk, 2.0)
                })
                .collect(),
            ..Default::default()
        };
        for id in 0..8 {
            let mut h = Humans::new(&f.root);
            h.walking.ped = Some(PedNet::build(&net));
            h.people.push(f.person(id, State::Standing, false));
            let at = point(2.0, 2.5);
            h.walk_street(0, at, 0.0, None, Some(&net));
            assert_eq!(h.people[0].position, at);
            let State::Strolling(mut walk) = h.people[0].state.clone() else {
                panic!()
            };
            let mut visited = HashSet::new();
            for _ in 0..50 {
                if walk.leg >= walk.legs.len() {
                    break;
                }
                let leg = walk.legs[walk.leg];
                assert!(
                    visited.insert(leg.lane),
                    "repeated tiny departure path {:?}",
                    walk.legs
                );
                h.people[0].position = leg.at(&net, leg.len()).0;
                walk.s = leg.len();
                h.walk_want(0, &mut walk, &net, None, &[], 0.05);
            }
            assert!(
                walk.leg >= walk.legs.len(),
                "departure must finish or leave the small network"
            );
            assert_eq!(
                h.walk_want(0, &mut walk, &net, None, &[], 0.05).vel,
                DVec2::ZERO
            );
        }
    }
}

#[test]
fn sitting_and_getting_up_keep_root_and_heading_continuous() {
    let f = Fixture::new();
    for stature in [1.3, 1.77, 2.1] {
        for height in [0.35, 0.65] {
            let mut h = Humans::new(&f.root);
            h.ik = true;
            let mut c = cabin();
            let seat = &mut Arc::get_mut(&mut c).unwrap().seats[1];
            seat.height = height;
            seat.pos.z = height;
            seat.rot = 90.0;
            let b = bus(c);
            let ix = [(b.id, 0)].into_iter().collect();
            let mut p = Pax::new(1.1);
            p.task = Task::InBusToPlace;
            p.inside = Some(b.id);
            p.bus = Some(b.id);
            p.seat = Some(1);
            p.pos = DVec3::Y * 2.0;
            p.yaw = std::f64::consts::PI;
            let mut person = f.person(7, State::Pax(Box::new(p)), false);
            person.ty = f.scaled_human(stature);
            person.position = DVec3::Y * 2.0;
            person.place = Place::Bus(b.id, Vec3::Y * 2.0);
            person.heading = 180.0;
            person.lheading = 180.0;
            h.people.push(person);
            h.animate(0.0, &f.world, std::slice::from_ref(&b), &ix);
            let initial = (h.pax(0).unwrap().pos, h.pax(0).unwrap().yaw);
            h.set_task(
                0,
                Task::SittingInBus,
                std::slice::from_ref(&b),
                &ix,
                &f.world,
            );
            assert_eq!(
                (h.pax(0).unwrap().pos, h.pax(0).unwrap().yaw),
                initial,
                "starting a sit must not teleport or snap yaw"
            );
            let dt = 1.0 / 30.0;
            let mut previous = initial;
            for _ in 0..150 {
                h.pax_frame(
                    dt,
                    &f.world,
                    None,
                    std::slice::from_ref(&b),
                    &ix,
                    &HashMap::new(),
                    None,
                    &mut |_, _, _, _| panic!("no payment"),
                    &mut false,
                    &mut vec![],
                );
                h.animate(dt, &f.world, std::slice::from_ref(&b), &ix);
                let p = h.pax(0).unwrap();
                assert!((p.pos - previous.0).length() <= 0.03);
                assert!(
                    crowd::angle_diff(previous.1.to_degrees(), p.yaw.to_degrees())
                        .to_radians()
                        .abs()
                        <= 0.08
                );
                previous = (p.pos, p.yaw);
                let ty = h.people[0].ty.clone();
                assert!(h.people[0].pose.bones(&ty.rig).ok);
            }
            assert!(h.people[0].pose.sit_amount() > 0.98);
            let seated = h.pax(0).unwrap().pos;
            let person = &h.people[0];
            let hip = person.render.active_bones.unwrap()[8].transform_point3(person.ty.rig.pelvis);
            let hip = seated.as_vec3()
                + Mat4::from_rotation_z(-person.lheading.to_radians() as f32)
                    .transform_vector3(hip);
            let seat = &b.cabin.seats[1];
            assert!(
                (hip.truncate() - seat.pos.truncate()).length() < 0.05,
                "stature {stature}, seat {height}: pelvis misses seat {hip:?}"
            );
            assert!((hip.z - (height + person.ty.rig.seat_lift)).abs() < 0.05);
            h.set_task(
                0,
                Task::InBusToExit,
                std::slice::from_ref(&b),
                &ix,
                &f.world,
            );
            assert_eq!(h.pax(0).unwrap().pos, seated);
            for _ in 0..45 {
                let getting_up = h.people[0].pose.sit_amount() > 0.02;
                h.pax_frame(
                    dt,
                    &f.world,
                    None,
                    std::slice::from_ref(&b),
                    &ix,
                    &HashMap::new(),
                    None,
                    &mut |_, _, _, _| panic!("no payment"),
                    &mut false,
                    &mut vec![],
                );
                if getting_up {
                    assert_eq!(h.pax(0).unwrap().pos, seated);
                }
                h.animate(dt, &f.world, std::slice::from_ref(&b), &ix);
            }
            assert!(h.people[0].pose.sit_amount() < 0.02);
        }
    }
}

#[test]
fn alighting_moves_the_authoritative_root_through_the_doorway_before_handoff() {
    let f = Fixture::new();
    for floor in [None, Some(0.15)] {
        if let Some(height) = floor {
            let mut terrain = ::map::Terrain::flat();
            terrain.heights.fill(height);
            f.world.terrains.write().insert((0, 0), Arc::new(terrain));
        }
        let mut h = Humans::new(&f.root);
        let mut c = cabin();
        Arc::get_mut(&mut c).unwrap().exits[0].outside = Vec3::new(1.5, 0.0, -0.8);
        let mut b = bus(c);
        b.pos.z = 0.8;
        b.accel.y = -6.0;
        let mut p = Pax::new(1.1);
        p.task = Task::InBusToExit;
        p.bus = Some(b.id);
        p.inside = Some(b.id);
        p.pt = Some(0);
        p.pt_target = Some(0);
        p.door = Some(0);
        p.movement = Movement::AtPathEnd;
        let mut person = f.person(7, State::Pax(Box::new(p)), false);
        person.position = b.pos;
        person.place = Place::Bus(b.id, Vec3::ZERO);
        h.people.push(person);
        let ix = [(b.id, 0)].into_iter().collect();
        for _ in 0..12 {
            h.animate(0.05, &f.world, std::slice::from_ref(&b), &ix);
        }
        let regs = [(
            b.id,
            pax::BusAtStops {
                next: Some(1),
                at: Some(1),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect();
        let mut previous = h.people[0].position;
        let mut frames_inside = 0;
        let mut frame_changes = 0;
        let mut was_inside = true;
        let mut previous_body: Option<DVec3> = None;
        for _ in 0..100 {
            h.pax_frame(
                0.05,
                &f.world,
                None,
                std::slice::from_ref(&b),
                &ix,
                &regs,
                None,
                &mut |_, _, _, _| panic!("no payment"),
                &mut false,
                &mut vec![],
            );
            h.animate(0.05, &f.world, std::slice::from_ref(&b), &ix);
            let current = h.people[0].position;
            let person = &h.people[0];
            let expected_origin = match person.place {
                Place::Bus(_, local) => local.as_dvec3(),
                Place::Ground => person.position,
            };
            assert_eq!(person.pose.floor_pose().1, expected_origin);
            let hip = person.render.active_bones.unwrap()[8].transform_point3(person.ty.rig.pelvis);
            let body = current
                + Mat4::from_rotation_z(-person.heading.to_radians() as f32)
                    .transform_vector3(hip)
                    .as_dvec3();
            if let Some(previous) = previous_body {
                assert!(
                    (body - previous).length() < 0.15,
                    "visible body jumped {previous:?} -> {body:?}"
                );
            }
            previous_body = Some(body);
            assert!(
                (current - previous).length() <= 0.06,
                "doorway root jumped {previous:?} -> {current:?}"
            );
            let inside = matches!(h.people[0].place, Place::Bus(..));
            frames_inside += usize::from(inside);
            frame_changes += usize::from(inside != was_inside);
            was_inside = inside;
            previous = current;
        }
        assert!(frames_inside > 10);
        assert_eq!(frame_changes, 1);
        assert!(matches!(h.people[0].state, State::Standing));
        let mut outside = b.world(b.cabin.exits[0].outside);
        if let Some(height) = floor {
            outside.z = height as f64;
        }
        assert!((h.people[0].position - outside).length() < 1e-6);
    }
}

#[test]
fn procedural_waiting_points_start_on_the_floor_and_keep_it_when_walking() {
    let f = Fixture::new();
    f.world
        .terrains
        .write()
        .insert((0, 0), Arc::new(::map::Terrain::flat()));
    for height in [0.0, 0.5] {
        let mut h = Humans::new(&f.root);
        h.ik = true;
        let mut s = stop(DVec3::new(2.0, 2.0, 0.0), 0.0, "Origin");
        s.spots[0].pos = DVec3::new(2.0, 2.0, if height == 0.0 { -0.25 } else { 0.5 });
        s.spots[0].height = height;
        h.stops.insert(1, s);
        let mut p = Pax::new(1.1);
        p.stop = Some(1);
        p.spot = Some(0);
        h.people.push(f.person(7, State::Pax(Box::new(p)), false));
        h.set_task(0, Task::WaitingForBus, &[], &HashMap::new(), &f.world);
        assert_eq!(
            h.pax(0).unwrap().pos.z,
            0.0,
            "final floor before the first visible frame"
        );
        assert_eq!(h.pax(0).unwrap().posture, Posture::Standing);
        for _ in 0..240 {
            h.pax_frame(
                0.05,
                &f.world,
                None,
                &[],
                &HashMap::new(),
                &HashMap::new(),
                None,
                &mut |_, _, _, _| panic!("no payment"),
                &mut false,
                &mut vec![],
            );
            h.animate(0.05, &f.world, &[], &HashMap::new());
            assert_eq!(h.people[0].position.z, 0.0);
            assert_eq!(h.pax(0).unwrap().pos, h.people[0].position);
        }
        assert_eq!(
            h.people[0].activity,
            if height == 0.0 {
                Activity::Stand
            } else {
                Activity::Sit
            }
        );
        let before = h.pax(0).unwrap().pos;
        h.set_task(0, Task::ToBus, &[], &HashMap::new(), &f.world);
        assert_eq!(h.pax(0).unwrap().pos.z, 0.0);
        h.pax_move(0, 0.05, 50.0, &f.world, &[], &HashMap::new());
        assert_eq!(h.pax(0).unwrap().pos.z, 0.0);
        if height != 0.0 {
            assert_eq!(h.pax(0).unwrap().pos, before, "get up before walking");
        }
    }
}

#[test]
fn streamed_floor_correction_changes_the_passenger_authority() {
    let f = Fixture::new();
    let mut p = Pax::new(1.1);
    p.pos = DVec3::new(2.0, 2.0, -0.25);
    let mut person = f.person(7, State::Pax(Box::new(p)), false);
    person.position = DVec3::new(2.0, 2.0, -0.25);
    person.set_ground_height(0.0, true);
    assert_eq!(person.position.z, 0.0);
    let State::Pax(p) = &person.state else {
        panic!()
    };
    assert_eq!(p.pos, person.position);
}

#[test]
fn releasing_a_fare_owner_clears_every_transaction_phase_but_not_currency() {
    let f = Fixture::new();
    for phase in [
        FarePhase::RequestTicket,
        FarePhase::Paying,
        FarePhase::AwaitTicket,
        FarePhase::TakingTicket,
        FarePhase::AwaitChange,
        FarePhase::TakingChange,
    ] {
        let mut h = Humans::new(&f.root);
        let mut p = Pax::new(1.1);
        p.task = Task::InBusToPlace;
        p.fare_phase = phase;
        h.people.push(f.person(7, State::Pax(Box::new(p)), false));
        h.desk.desk_busy = Some(7);
        h.request = Some(("Single".into(), 2.5));
        h.paid = Some((5.0, 2.5));
        h.change_due = Some(2.5);
        h.release(0);
        assert!(h.desk.desk_busy.is_none() && h.request.is_none());
        assert!(
            h.paid.is_none() && h.change_due.is_none(),
            "phase {phase:?}"
        );
    }
}

#[test]
fn mirror_mode_remaps_all_retained_person_owners_and_keeps_the_sale_live() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut p = Pax::new(1.1);
    p.task = Task::InBusToPlace;
    p.bus = Some(BusId::Player);
    p.inside = Some(BusId::Player);
    p.fare_phase = FarePhase::AwaitTicket;
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
    let mut avatar = f.person(8, State::Idle, false);
    avatar.puppet = Some(Puppet {
        mode: PuppetMode::Avatar,
    });
    h.people.push(avatar);
    h.avatars.avatars.insert(99, 8);
    h.avatars.avatar_hidden.insert(8, true);
    h.desk.desk_busy = Some(7);
    h.request = Some(("Single".into(), 2.5));
    h.paid = Some((5.0, 2.5));
    h.set_mirror(true);
    assert_eq!(h.desk.desk_busy, Some(h.people[0].id));
    assert_eq!(h.avatars.avatars[&99], h.people[1].id);
    assert_eq!(h.avatars.avatar_hidden.get(&h.people[1].id), Some(&true));
    assert!(h.people.iter().all(|p| p.id >= 1 << 30));
    assert_eq!(h.request.as_ref().unwrap().0, "Single");
    assert_eq!(h.paid, Some((5.0, 2.5)));
    let ids = h.people.iter().map(|p| p.id).collect::<Vec<_>>();
    h.set_mirror(false);
    h.set_mirror(true);
    assert_eq!(h.people.iter().map(|p| p.id).collect::<Vec<_>>(), ids);
    let avatar_id = h.avatars.avatars[&99];
    h.avatar_remove(99);
    assert!(!h.avatars.avatar_hidden.contains_key(&avatar_id));
}

#[test]
fn missing_waiting_spot_keeps_a_grant_retryable_without_consuming_the_mirror() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    h.people.push(f.person(7, State::Standing, true));
    h.stops.insert(1, stop(DVec3::ZERO, 0.0, "Origin"));
    h.network.mirror_wait.insert(7, (1, 1));
    let grant = PassengerGrant {
        id: 7,
        transfer: 1,
        stop: 1,
        spot: 1,
        destination: None,
        alternative: None,
        alternative_m: 0.0,
        ride_km: 5.0,
        line_destination: None,
        allowed_termini: None,
    };
    assert!(!h.grant(&grant));
    assert!(h.people[0].remote);
    assert_eq!(h.network.mirror_wait[&7], (1, 1));
    let s = h.stops.get_mut(&1).unwrap();
    s.spots.push(s.spots[0].clone());
    s.taken.push(false);
    assert!(h.grant(&grant));
    assert!(h.grant(&grant));
    assert_eq!(h.people.len(), 1);
    assert!(h.stops[&1].taken[1]);
}

impl Fixture {
    fn human_at(&self, file: &str, weight: Option<f32>) -> Arc<HumanType> {
        let path = self.root.join(file);
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        std::fs::write(parent.join("model.cfg"), "").unwrap();
        let mut definition = std::fs::read_to_string(self.root.join("person.hum")).unwrap();
        if let Some(weight) = weight {
            definition.push_str(&format!("\n[neo_weight]\n{weight}\n"));
        }
        std::fs::write(&path, definition).unwrap();
        Arc::new(HumanType::load(&path).unwrap())
    }

    fn scaled_human(&self, stature: f32) -> Arc<HumanType> {
        let path = self.root.join(format!("scaled-{stature}.hum"));
        let links = [
            0.09, 0.0, 0.92, 0.09, -0.03, 0.53, 0.02, 1.17, 0.18, -0.05, 1.43, 0.44, -0.04, 1.41,
            -0.02, 1.55, 0.69, -0.03, 1.43, 0.9, -0.03, 1.43,
        ];
        let scale = stature / 1.77;
        let links = links
            .iter()
            .map(|v| format!("{}\n", v * scale))
            .collect::<String>();
        std::fs::write(&path, format!("[model]\nmodel.cfg\n[humangeom]\n0.18\n{stature}\n[seatheight]\n{}\n[links]\n{links}",0.82*scale)).unwrap();
        Arc::new(HumanType::load(&path).unwrap())
    }
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "omsi-pax-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("global.cfg"), "[name]\nPassenger tests\n").unwrap();
        std::fs::write(root.join("person.hum"), "[model]\nmodel.cfg\n[humangeom]\n0.18\n1.77\n[seatheight]\n0.82\n[links]\n0.09\n0\n0.92\n0.09\n-0.03\n0.53\n0.02\n1.17\n0.18\n-0.05\n1.43\n0.44\n-0.04\n1.41\n-0.02\n1.55\n0.69\n-0.03\n1.43\n0.9\n-0.03\n1.43\n").unwrap();
        std::fs::write(root.join("model.cfg"), "").unwrap();
        let world = World::open(&root, &root.join("global.cfg"), 19890530).unwrap();
        let human = Arc::new(HumanType::load(&root.join("person.hum")).unwrap());
        Self { root, world, human }
    }

    fn person(&self, id: u32, state: State, remote: bool) -> Person {
        Person {
            render: PersonRender {
                level: 0,
                blob: None,
                blob_shown: false,
                mirror_seat: None,
                active_bones: None,
                meshes: vec![],
                skins: vec![],
                skin_bones: None,
                pose_changed: false,
                lit: 0.0,
                skinned: false,
                since_posed: 0,
                posed_at: (DVec3::ZERO, 0.0),
                ankles: [Vec3::ZERO; 2],
            },
            id,
            ty: self.human.clone(),
            variant: 0,
            position: DVec3::ZERO,
            heading: 0.0,
            lheading: 0.0,
            place: Place::Ground,
            vel: DVec2::ZERO,
            pace: 1.1,
            activity: Activity::Stand,
            anim: OmsiAnim::default(),
            pose: ::simulation::human::Pose::new(id),
            state,
            t_state: 0.0,
            interior: 0.0,
            tilt: Mat4::IDENTITY,
            age: 40.0,
            stuck: 0.0,
            ghost: 0.0,
            car_wait: 0.0,
            detour: 0.0,
            detour_side: 0.0,
            why: "",
            puppet: None,
            remote,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

pub(super) fn cabin() -> Arc<Cabin> {
    let points = vec![
        Vec3::ZERO,
        Vec3::Y,
        Vec3::Y * 2.0,
        Vec3::Y * 3.0,
        Vec3::X * 20.0,
    ];
    let links = vec![(0, 1, false), (1, 2, false), (2, 3, false)];
    let graph = PathGraph::new(points.clone(), &links);
    let door = |point| Door {
        inside: points[point],
        point: Some(point),
        outside: points[point],
        side: 1.0,
        queue_dir: 1.0,
        sells: true,
        button: false,
        wait: points[point],
    };
    let seat = |point, seated| Seat {
        point: Some(point),
        pos: points[point] + if seated { Vec3::Z * 0.5 } else { Vec3::ZERO },
        floor: points[point],
        rot: 0.0,
        seated,
        height: if seated { 0.5 } else { 0.0 },
        omsi_seat: point,
    };
    Arc::new(Cabin {
        data: PassengerCabin::default(),
        routes: pax::build_routes(&graph, &links),
        graph,
        links,
        link_pack: vec![None; 3],
        step_packs: vec![],
        entries: vec![door(0)],
        exits: vec![door(0), door(3)],
        desk: None,
        seats: vec![seat(1, false), seat(2, true), seat(4, true)],
        parts: vec![CabinPart {
            offset: Vec3::ZERO,
            joint_y: f32::INFINITY,
        }],
        link_room: vec![2.0; 3],
        stamper: None,
        sale: None,
        money_point: None,
        money_var: None,
        change_point: None,
        money_parent: None,
        change_parent: None,
    })
}

fn bus(cabin: Arc<Cabin>) -> BusNow {
    BusNow {
        id: BusId::Player,
        entry_open: vec![true],
        exit_open: vec![true, true],
        cabin,
        pos: DVec3::ZERO,
        rot: Mat4::IDENTITY,
        heading: 0.0,
        speed: 0.0,
        walk_open: None,
        interior: 0.0,
        air: CabinAir::default(),
        half: DVec2::new(1.25, 6.0),
        centre: DVec2::ZERO,
        accel: DVec2::ZERO,
        trailers: vec![],
        terminus: None,
        out_of_service: false,
    }
}

fn set_population(h: &mut Humans, types: impl IntoIterator<Item = Arc<HumanType>>) {
    for ty in types {
        let index = h.type_index(ty);
        h.population.push(index);
    }
    h.map_humans_done = true;
}

#[test]
fn registering_a_weightless_lan_alternate_does_not_add_a_local_spawn_weight() {
    let f = Fixture::new();
    let base = f.human_at("Humans/Other/Man.hum", None);
    let alternate = f.human_at("Humans/Other/Man~Alt.hum", Some(0.0));
    let mut h = Humans::new(&f.root);
    h.types.clear();
    h.population.clear();
    h.alternates.clear();
    set_population(&mut h, [base.clone()]);
    h.alternates
        .insert(figures::slot_key(&base.def.path), vec![alternate.clone()]);

    let index = h
        .type_by_file(&Humans::type_file(&alternate))
        .expect("registered LAN figure");

    assert_eq!(index, 1);
    assert_eq!(h.population, [0]);
    assert!(Arc::ptr_eq(&h.types[index], &alternate));
    for _ in 0..64 {
        let (picked, _) = h.pick_figure(DVec3::ZERO, None);
        assert!(Arc::ptr_eq(&picked, &base));
    }
    let (forced, _) = h.pick_figure(DVec3::ZERO, Some(index));
    assert!(Arc::ptr_eq(&forced, &alternate));
}

#[test]
fn map_population_keeps_duplicate_weights_explicit_alternates_and_registry_indices() {
    let f = Fixture::new();
    let base_a = f.human_at("Humans/Test/A.hum", None);
    let base_b = f.human_at("Humans/Test/B.hum", None);
    let group_alt = f.human_at("Humans/Test/B~Group.hum", Some(1.0));
    let explicit_alt = f.human_at("Humans/Test/B~Blue.hum", Some(1.0));
    let mut h = Humans::new(&f.root);
    h.types.clear();
    h.population.clear();
    h.alternates.clear();
    h.types.extend([base_a.clone(), base_b.clone()]);
    h.population.extend([0, 1]);
    let group_index = h.type_index(group_alt.clone());
    h.alternates
        .insert(figures::slot_key(&base_b.def.path), vec![group_alt.clone()]);
    std::fs::write(
        f.world.map_dir.join("humans.txt"),
        "Humans/Test/A.hum\nHumans/Test/A.hum\nHumans/Test/B.hum\nHumans/Test/B~Blue.hum\n",
    )
    .unwrap();

    h.use_map_humans(&f.world);

    assert_eq!(group_index, 2);
    assert_eq!(h.population, [0, 0, 1, 3]);
    assert!(Arc::ptr_eq(&h.types[0], &base_a));
    assert!(Arc::ptr_eq(&h.types[1], &base_b));
    assert!(Arc::ptr_eq(&h.types[2], &group_alt));
    assert_eq!(
        Humans::type_file(&h.types[3]),
        Humans::type_file(&explicit_alt),
        "the map's explicit alternate stays the selected model"
    );
    assert_eq!(h.type_index(group_alt), group_index);
    assert_eq!(h.type_index(explicit_alt.clone()), 3);

    h.population = vec![3];
    let (picked, _) = h.pick_figure(DVec3::ZERO, None);
    assert_eq!(
        Humans::type_file(&picked),
        Humans::type_file(&explicit_alt),
        "explicit alternate population slots do not sample their group's alternatives"
    );
}

#[test]
fn avatar_type_selection_uses_weighted_population_not_registry_length() {
    let f = Fixture::new();
    let base_a = f.human_at("Humans/Test/A.hum", None);
    let base_b = f.human_at("Humans/Test/B.hum", None);
    let lan_alt = f.human_at("Humans/Test/B~Lan.hum", Some(0.0));
    let mut h = Humans::new(&f.root);
    h.types.clear();
    h.population.clear();
    h.alternates.clear();
    h.types.extend([base_a, base_b.clone()]);
    h.population.extend([0, 0, 1]);
    h.map_humans_done = true;
    assert_eq!(h.type_index(lan_alt.clone()), 2);
    let renderer = noop_renderer();
    let mut scene = renderer.new_scene();

    h.avatar(
        1,
        &f.world,
        &renderer,
        &mut scene,
        AvatarCmd {
            pos: DVec3::ZERO,
            heading: 0.0,
            vel: DVec2::ZERO,
            lift: 0.0,
            seat: None,
            floor: None,
            aboard: None,
            wheel: None,
        },
        2,
    );

    assert_eq!(h.people.len(), 1);
    assert!(Arc::ptr_eq(&h.people[0].ty, &base_b));
    assert!(!Arc::ptr_eq(&h.people[0].ty, &lan_alt));
}

fn stop(pos: DVec3, heading: f64, name: &str) -> PaxStop {
    PaxStop {
        name: name.into(),
        alias: String::new(),
        pos,
        heading,
        gather: pos,
        spots: vec![pax::WaitSpot {
            pos,
            face: 0.0,
            height: 0.0,
        }],
        taken: vec![false],
        enter_max: 1.0,
        enter_min: 1.0,
        length: 30.0,
        lane: None,
        was_near: false,
        near: false,
        clock_ms: 0.0,
        want: 1,
        factor: 1.0,
        buses: vec![],
        dests: vec![],
        lines: vec![],
    }
}

#[test]
fn reachable_seats_are_preferred_and_reservations_are_unique() {
    let mut h = Humans::new(Path::new("/nonexistent"));
    h.prefer_seats = true;
    let c = cabin();
    assert_eq!(h.reserve_place(BusId::Player, &c, None, false), Some(1));
    assert_eq!(h.reserve_place(BusId::Player, &c, None, false), Some(0));
    assert_eq!(
        h.reserve_place(BusId::Player, &c, None, false),
        None,
        "isolated seat stays unused"
    );
    h.free_seat(BusId::Player, 1);
    assert_eq!(h.reserve_place(BusId::Player, &c, Some(1), true), Some(1));
}

#[test]
fn natural_entry_queues_add_load_and_retain_a_walkers_current_door() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let person = |id, door| {
        let mut pax = Pax::new(1.1);
        pax.task = Task::WalkingToBus;
        pax.bus = Some(BusId::Player);
        pax.door = door;
        f.person(id, State::Pax(Box::new(pax)), false)
    };
    h.people.push(person(1, Some(0)));
    let retained = h.door_queues(0, BusId::Player, 2);
    h.pax_mut(0).unwrap().door = None;
    let without_hysteresis = h.door_queues(0, BusId::Player, 2);
    assert!((retained[0] - (without_hysteresis[0] - 1.5)).abs() < 1e-6);

    h.people.push(person(2, Some(0)));
    let with_queue = h.door_queues(0, BusId::Player, 2);
    assert!((with_queue[0] - (without_hysteresis[0] + 2.0)).abs() < 1e-6);
    assert_eq!(with_queue[1], without_hysteresis[1]);
}

#[test]
fn a_complete_journey_validates_rides_requests_an_exit_and_joins_the_sidewalk() {
    complete_journey(TicketAction::Stamp);
}

#[test]
fn a_complete_journey_buys_an_exact_fare_ticket_and_joins_the_sidewalk() {
    complete_journey(TicketAction::Buy);
}

#[test]
fn a_complete_journey_without_fare_devices_reaches_the_sidewalk() {
    complete_journey(TicketAction::None);
}

#[test]
fn manual_ticket_sale_waits_for_the_requested_ticket_and_keeps_rating_counters() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    h.boarding = "pay".into();
    h.tickets = Some(Arc::new(::content::TicketPack {
        tickets: vec![::content::Ticket {
            name: "Single".into(),
            value: 2.5,
            ..Default::default()
        }],
        ..Default::default()
    }));
    let b = bus(cabin());
    let mut p = Pax::new(1.1);
    p.task = Task::InBusToPlace;
    p.inside = Some(b.id);
    p.bus = Some(b.id);
    p.seat = Some(1);
    p.ticket = TicketAction::Buy;
    p.ticket_id = 1;
    p.price = 2.5;
    p.fare_phase = FarePhase::RequestTicket;
    h.people.push(f.person(42, State::Pax(Box::new(p)), false));
    let mut taken = false;
    let mut sale = |h: &mut Humans, ticket: Option<f32>| {
        h.pax_mut(0).unwrap().timer = 0.0;
        h.desk_sale(
            0,
            &b,
            ticket,
            &mut |_, _, _, _| panic!("no money geometry in this cabin"),
            &mut taken,
        );
    };
    sale(&mut h, None);
    assert_eq!(h.desk.desk_busy, Some(42));
    sale(&mut h, None);
    assert_eq!(h.pax(0).unwrap().fare_phase, FarePhase::AwaitTicket);
    sale(&mut h, Some(1.0));
    assert_eq!(
        h.pax(0).unwrap().fare_phase,
        FarePhase::AwaitTicket,
        "wrong ticket does not complete the sale"
    );
    sale(&mut h, Some(0.0));
    assert_eq!(h.pax(0).unwrap().fare_phase, FarePhase::TakingTicket);
    sale(&mut h, None);
    sale(&mut h, None);
    assert_eq!(
        (h.ticket_requests, h.tickets_sold, h.ticket_cash, h.served),
        (1, 1, 2.5, 1)
    );
    assert_eq!(h.pax(0).unwrap().ticket, TicketAction::None);
    assert!(h.desk.desk_busy.is_none());

    // A correct change tray earns two points; an exact fare has no change transaction.
    h.pax_mut(0).unwrap().fare_phase = FarePhase::AwaitChange;
    sale(&mut h, None);
    assert_eq!(h.ticket_points, 2);
    assert!(taken);
}

fn complete_journey(fare: TicketAction) {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut c = cabin();
    if fare == TicketAction::Stamp {
        Arc::get_mut(&mut c).unwrap().stamper = Some((Some(1), Vec3::Y));
    } else if fare == TicketAction::Buy {
        let cabin = Arc::get_mut(&mut c).unwrap();
        cabin.sale = Some((Some(1), Vec3::Y));
        cabin.money_var = Some((Vec3::Y, [0.1, 0.1]));
        let mut money = crate::money::Money::new(&f.root, "synthetic-money.cfg");
        money.currency = Some(::content::Currency {
            coins: vec![("one.o3d".into(), 1.0), ("half.o3d".into(), 0.5)],
            ..Default::default()
        });
        h.money = Some(money);
    }
    let mut b = bus(c);
    b.entry_open[0] = false;
    b.exit_open = vec![false; 2];
    Arc::get_mut(&mut b.cabin).unwrap().entries[0].button = true;
    h.tickets = Some(Arc::new(::content::tickets::TicketPack {
        stamper_prop: if fare == TicketAction::Stamp {
            1.0
        } else {
            0.0
        },
        ticketbuy_prop: if fare == TicketAction::Buy { 1.0 } else { 0.0 },
        tickets: vec![::content::Ticket {
            name: "Single".into(),
            value: 2.5,
            age_max: 99,
            probability: 1.0,
            ..Default::default()
        }],
        ..Default::default()
    }));
    let net = Network {
        lanes: vec![::traffic::LaneBuilder::polyline(
            vec![DVec3::ZERO, DVec3::Y * 50.0],
            LaneKind::Sidewalk,
            2.0,
        )],
        ..Default::default()
    };
    h.walking.ped = Some(PedNet::build(&net));
    let mut origin = stop(DVec3::ZERO, 0.0, "Origin");
    origin.buses = vec![(b.id, true)];
    origin.taken[0] = true;
    h.stops.insert(1, origin);
    let mut destination = stop(DVec3::Y * 3.0, 0.0, "Destination");
    destination.lane = Some((0, 3.0));
    h.stops.insert(2, destination);
    let mut p = Pax::new(1.1);
    p.task = Task::WaitingForBus;
    p.stop = Some(1);
    p.spot = Some(0);
    p.journey.dest = Some("Destination".into());
    h.people.push(f.person(42, State::Pax(Box::new(p)), false));
    let ix = [(b.id, 0)].into_iter().collect();
    let mut regs = [(
        b.id,
        pax::BusAtStops {
            next: Some(1),
            at: Some(1),
            near: vec![1],
            ..Default::default()
        },
    )]
    .into_iter()
    .collect::<HashMap<_, _>>();
    let mut removed = vec![];
    let mut taken_ticket = false;
    let mut payments = vec![];
    let mut tick = |h: &mut Humans, b: &BusNow, regs: &HashMap<_, _>| {
        h.pax_frame(
            0.05,
            &f.world,
            Some(&net),
            std::slice::from_ref(b),
            &ix,
            regs,
            None,
            &mut |money, coins, _, _| payments.push(money.value_of(coins)),
            &mut taken_ticket,
            &mut removed,
        );
        if let Some(p) = h.pax(0) {
            let (position, heading) = h.pax_world(p, std::slice::from_ref(b), &ix).unwrap();
            assert_eq!(h.people[0].position, position);
            assert_eq!(h.people[0].heading, heading);
            assert_eq!(
                h.people[0].place,
                match p.inside {
                    Some(bus) => Place::Bus(bus, p.pos.as_vec3()),
                    None => Place::Ground,
                }
            );
        }
    };
    tick(&mut h, &b, &regs);
    assert_eq!(h.pax(0).unwrap().task, Task::ToBus);
    assert_eq!(h.pax(0).unwrap().ticket, fare);
    assert!(!h.stops[&1].taken[0]);
    tick(&mut h, &b, &regs);
    let reserved = h.pax(0).unwrap().seat.unwrap();
    assert!(h.buses.seats[&b.id][reserved]);
    for _ in 0..200 {
        tick(&mut h, &b, &regs);
        if h.entry_req[0] {
            break;
        }
    }
    assert!(h.entry_req[0]);
    assert_eq!(
        h.pax(0).unwrap().inside,
        None,
        "closed door holds the passenger outside"
    );
    b.entry_open[0] = true;
    for _ in 0..600 {
        tick(&mut h, &b, &regs);
        if h.pax(0).unwrap().task == Task::SittingInBus {
            break;
        }
    }
    assert_eq!(h.pax(0).unwrap().task, Task::SittingInBus);
    assert_eq!(h.pax(0).unwrap().seat, Some(reserved));
    assert_eq!(h.pax(0).unwrap().ticket, TicketAction::None);
    assert_eq!(
        h.stamped,
        if fare == TicketAction::Stamp {
            vec![b.id]
        } else {
            vec![]
        }
    );
    b.exit_open = vec![true; 2];
    regs.get_mut(&b.id).unwrap().next = Some(2);
    regs.get_mut(&b.id).unwrap().at = None;
    tick(&mut h, &b, &regs);
    assert!(h.stop_request);
    assert_eq!(h.pax(0).unwrap().task, Task::InBusToExit);
    assert_eq!(h.buses.seats[&b.id][reserved], b.cabin.seats[reserved].seated);
    regs.get_mut(&b.id).unwrap().at = Some(2);
    for _ in 0..600 {
        tick(&mut h, &b, &regs);
        if h.pax(0).is_none() {
            break;
        }
    }
    assert!(!h.buses.seats[&b.id][reserved]);
    assert!(matches!(h.people[0].state, State::Strolling(_)));
    assert_eq!(h.people[0].place, Place::Ground);
    assert_eq!(h.people[0].id, 42);
    assert_eq!(h.people.len(), 1);
    assert!(removed.is_empty());
    if fare == TicketAction::Buy {
        assert!(taken_ticket);
        assert_eq!(payments, vec![2.5]);
        assert_eq!(
            (h.ticket_requests, h.tickets_sold, h.ticket_cash, h.served),
            (1, 1, 2.5, 1)
        );
        assert!(h.request.is_none() && h.change_due.is_none() && h.desk.desk_busy.is_none());
    } else {
        assert!(!taken_ticket);
        assert!(payments.is_empty());
    }
}

#[test]
fn an_open_reachable_exit_wins_over_a_nearer_closed_one() {
    let c = cabin();
    assert_eq!(c.nearest_exit(1, &[false, true]), Some((1, 3)));
    assert_eq!(c.nearest_exit(1, &[true, true]), Some((0, 0)));
    assert_eq!(c.nearest_exit(4, &[true, true]), None);
}

#[test]
fn nearest_same_direction_stop_wins_independently_of_map_ids() {
    let mut h = Humans::new(Path::new("/nonexistent"));
    h.stops.insert(1, stop(DVec3::Y, 0.0, "Here"));
    h.stops.insert(99, stop(DVec3::Y * 40.0, 0.0, "Further"));
    h.stops
        .insert(100, stop(DVec3::new(0.0, 0.1, 0.0), 180.0, "Opposite"));
    let mut b = bus(cabin());
    b.terminus = Some("Opposite".into());
    let r = h
        .register_buses(&[b.clone()], 0.1)
        .remove(&BusId::Player)
        .unwrap();
    assert_eq!((r.next, r.at, r.all_exit), (Some(1), Some(1), false));
    b.terminus = None;
    assert!(
        !h.register_buses(&[b.clone()], 0.1)[&BusId::Player].all_exit,
        "unknown destination is not out of service"
    );
    b.out_of_service = true;
    assert!(h.register_buses(&[b], 0.1)[&BusId::Player].all_exit);
}

#[test]
fn an_unknown_terminus_keeps_service_distinct_from_explicit_all_exit() {
    let mut h = Humans::new(Path::new("/nonexistent"));
    h.stops.insert(1, stop(DVec3::ZERO, 0.0, "Origin"));
    let mut b = bus(cabin());
    assert!(b.terminus.is_none());
    let regs = h.register_buses(std::slice::from_ref(&b), 0.05);
    assert!(!regs[&b.id].all_exit);
    assert_eq!(h.stops[&1].buses, vec![(b.id, true)]);
    b.out_of_service = true;
    let regs = h.register_buses(std::slice::from_ref(&b), 0.05);
    assert!(regs[&b.id].all_exit);
    assert!(h.stops[&1].buses.is_empty());
}

#[test]
fn a_seat_being_vacated_still_counts_for_the_scripts() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    h.buses.player_cabin = Some(cabin());
    let mut p = Pax::new(1.1);
    p.task = Task::InBusToExit;
    p.bus = Some(BusId::Player);
    p.inside = Some(BusId::Player);
    p.vacating = Some(1);
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
    assert_eq!(h.seat_counts()[2], 1, "still getting up");
    h.pax_mut(0).unwrap().vacating = None;
    assert_eq!(h.seat_counts()[2], 0);
}

#[test]
fn opening_another_exit_preserves_the_current_path_progress() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    h.set_natural(false);
    let mut b = bus(cabin());
    b.exit_open = vec![false, true];
    let mut p = Pax::new(1.1);
    p.task = Task::InBusToExit;
    p.bus = Some(b.id);
    p.inside = Some(b.id);
    p.pt = Some(1);
    p.pt_target = Some(0);
    p.door = Some(0);
    p.movement = Movement::AlongPath;
    p.pos = DVec3::Y * 0.6;
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
    let ix = [(b.id, 0)].into_iter().collect();
    let regs = [(
        b.id,
        pax::BusAtStops {
            next: Some(1),
            at: Some(1),
            ..Default::default()
        },
    )]
    .into_iter()
    .collect();
    h.task_to_exit(0, 0.05, &[b], &ix, &regs, &f.world, None);
    let p = h.pax(0).unwrap();
    assert_eq!((p.pt, p.pt_target, p.door), (Some(1), Some(3), Some(1)));
    assert_eq!(p.pos, DVec3::Y * 0.6, "no snapping back to a path point");
}

#[test]
fn natural_closed_exit_wait_holds_its_reachable_path_then_uses_an_open_exit() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut b = bus(cabin());
    b.exit_open = vec![false, true];
    let mut p = Pax::new(1.1);
    p.task = Task::Nothing;
    p.bus = Some(b.id);
    p.inside = Some(b.id);
    p.door_wait = 3.0;
    p.pos = b.cabin.graph.points[1].as_dvec3();
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
    let ix = [(b.id, 0)].into_iter().collect();
    let regs = [(
        b.id,
        pax::BusAtStops {
            next: Some(1),
            at: Some(1),
            ..Default::default()
        },
    )]
    .into_iter()
    .collect();
    h.set_task(0, Task::InBusToExit, &[b.clone()], &ix, &f.world);
    assert_eq!(h.pax(0).unwrap().door_wait, 0.0);
    {
        let pax = h.pax_mut(0).unwrap();
        pax.pt = Some(1);
        pax.pt_target = Some(0);
        pax.door = Some(0);
        pax.movement = Movement::AlongPath;
        pax.timer = 0.0;
    }

    h.task_to_exit(0, 0.05, &[b.clone()], &ix, &regs, &f.world, None);
    assert_eq!(
        (
            h.pax(0).unwrap().pt,
            h.pax(0).unwrap().pt_target,
            h.pax(0).unwrap().door
        ),
        (Some(1), Some(0), Some(0)),
        "keep approaching the reachable closed exit"
    );

    {
        let pax = h.pax_mut(0).unwrap();
        pax.pt = Some(0);
        pax.pt_target = Some(0);
        pax.door = Some(0);
        pax.movement = Movement::AtPathEnd;
        pax.pos = b.cabin.graph.points[0].as_dvec3();
    }
    for waited in 1..5 {
        h.pax_mut(0).unwrap().timer = 0.0;
        h.task_to_exit(0, 0.05, &[b.clone()], &ix, &regs, &f.world, None);
        let pax = h.pax(0).unwrap();
        assert_eq!((pax.pt_target, pax.door), (Some(0), Some(0)));
        assert_eq!(pax.door_wait, waited as f32);
    }

    h.pax_mut(0).unwrap().timer = 0.0;
    h.task_to_exit(0, 0.05, &[b], &ix, &regs, &f.world, None);
    let pax = h.pax(0).unwrap();
    assert_eq!(
        (pax.pt, pax.pt_target, pax.door),
        (Some(0), Some(3), Some(1))
    );
    assert_eq!(pax.movement, Movement::AlongPath);
}

#[test]
fn a_stop_request_and_open_door_result_in_one_completed_exit() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let b = bus(cabin());
    let mut p = Pax::new(1.1);
    p.task = Task::InBusToExit;
    p.bus = Some(b.id);
    p.inside = Some(b.id);
    p.pt = Some(3);
    p.pt_target = Some(3);
    p.door = Some(1);
    p.movement = Movement::AtPathEnd;
    p.pos = DVec3::Y * 3.0;
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
    let ix = [(b.id, 0)].into_iter().collect();
    let regs = [(
        b.id,
        pax::BusAtStops {
            next: Some(1),
            at: Some(1),
            ..Default::default()
        },
    )]
    .into_iter()
    .collect();
    h.task_to_exit(0, 0.05, &[b], &ix, &regs, &f.world, None);
    assert!(matches!(h.people[0].state, State::Standing));
    assert_eq!(h.people[0].place, Place::Ground);
    assert_eq!(h.people.len(), 1);
}

#[test]
fn aborted_boarding_releases_its_place_and_bus_reference() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let b = bus(cabin());
    let mut p = Pax::new(1.1);
    p.task = Task::WalkingToBus;
    p.bus = Some(b.id);
    p.seat = Some(1);
    p.stop = Some(1);
    h.buses.seats.insert(b.id, vec![false, true, false]);
    h.stops.insert(1, stop(DVec3::ZERO, 0.0, "Test"));
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
    h.set_task(
        0,
        Task::WalkingToBusstop,
        &[b.clone()],
        &[(b.id, 0)].into_iter().collect(),
        &f.world,
    );
    assert!(!h.buses.seats[&b.id][1]);
    assert_eq!(
        (h.pax(0).unwrap().seat, h.pax(0).unwrap().bus),
        (None, None)
    );
}

fn timetable_bus_holds(h: &mut Humans, f: &Fixture, b: &BusNow) -> Vec<(u64, f32, bool)> {
    h.pax_frame(
        0.05,
        &f.world,
        None,
        std::slice::from_ref(b),
        &[(b.id, 0)].into_iter().collect(),
        &HashMap::new(),
        None,
        &mut |_, _, _, _| {},
        &mut false,
        &mut vec![],
    );
    h.take_holds()
}

fn timetable_bus_at_stop(f: &Fixture, h: &mut Humans, task: Task) -> BusNow {
    let mut b = bus(cabin());
    b.id = BusId::Ai(7);
    let mut origin = stop(DVec3::ZERO, 0.0, "Origin");
    origin.buses = vec![(b.id, true)];
    h.stops.insert(1, origin);
    let mut p = Pax::new(1.1);
    p.task = task;
    p.bus = Some(b.id);
    p.stop = Some(1);
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
    b
}

#[test]
fn nobody_left_without_a_place_in_a_full_timetable_bus_holds_it() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let b = timetable_bus_at_stop(&f, &mut h, Task::ToBus);
    h.buses.seats.insert(b.id, vec![true; 3]);
    assert!(timetable_bus_holds(&mut h, &f, &b).is_empty());
    assert_eq!(h.pax(0).unwrap().task, Task::ToBus);
    assert_eq!(h.pax(0).unwrap().seat, None);
    h.stops.get_mut(&1).unwrap().buses.clear();
    timetable_bus_holds(&mut h, &f, &b);
    assert_eq!(h.pax(0).unwrap().task, Task::WalkingToBusstop);
    assert_eq!(h.pax(0).unwrap().bus, None);
}

#[test]
fn a_boarder_holds_a_timetable_bus_until_giving_up_at_a_shut_door() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut b = timetable_bus_at_stop(&f, &mut h, Task::WalkingToBus);
    b.entry_open[0] = false;
    b.exit_open = vec![false; 2];
    h.buses.seats.insert(b.id, vec![false, true, false]);
    h.pax_mut(0).unwrap().seat = Some(1);
    assert_eq!(timetable_bus_holds(&mut h, &f, &b), [(7, 2.5, false)]);
    h.pax_mut(0).unwrap().door_wait = 60.0;
    assert!(timetable_bus_holds(&mut h, &f, &b).is_empty());
    h.pax_mut(0).unwrap().door = Some(0);
    b.entry_open[0] = true;
    assert_eq!(
        timetable_bus_holds(&mut h, &f, &b),
        [(7, 2.5, false)],
        "the door opened after all: boarding again"
    );
}

#[test]
fn somebody_crossing_a_doorway_holds_a_timetable_bus_as_in_the_doorway() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let b = timetable_bus_at_stop(&f, &mut h, Task::InBusToExit);
    let p = h.pax_mut(0).unwrap();
    p.inside = Some(b.id);
    p.door = Some(0);
    assert_eq!(timetable_bus_holds(&mut h, &f, &b), [(7, 2.5, false)]);
    let p = h.pax_mut(0).unwrap();
    p.pos = DVec3::ZERO;
    p.doorway = Some(Doorway {
        target: Vec3::X * 10.0,
        stop: Some(1),
    });
    assert_eq!(timetable_bus_holds(&mut h, &f, &b), [(7, 2.5, true)]);
}

#[test]
fn lan_grants_preserve_journeys_and_do_not_consume_missing_metadata() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    h.people.push(f.person(7, State::Standing, true));
    h.network.mirror_wait.insert(7, (1, 0));
    let grant = PassengerGrant {
        id: 7,
        transfer: 1,
        stop: 1,
        spot: 0,
        destination: Some("Königsrath".into()),
        alternative: Some("Pause".into()),
        alternative_m: 500.0,
        ride_km: 12.0,
        line_destination: Some("Trip".into()),
        allowed_termini: Some(vec!["Destination".into()]),
    };
    assert!(!h.grant(&grant));
    assert!(h.network.mirror_wait.contains_key(&7));
    let mut s = stop(DVec3::ZERO, 0.0, "Test");
    s.lines = vec![
        ("Other".into(), HashSet::new()),
        ("Trip".into(), HashSet::new()),
    ];
    h.stops.insert(1, s);
    assert!(h.grant(&grant));
    assert!(h.grant(&grant), "duplicate receipt is idempotent");
    assert_eq!(h.people.len(), 1);
    let p = h.pax(0).unwrap();
    assert_eq!(
        (
            p.journey.dest.as_deref(),
            p.journey.alt.as_deref(),
            p.journey.ride_km,
            p.journey.line
        ),
        (Some("Königsrath"), Some("Pause"), 12.0, Some(1))
    );
}

#[test]
fn a_client_without_timetable_records_keeps_the_hosts_line_permission() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    h.people.push(f.person(7, State::Standing, true));
    let mut s = stop(DVec3::ZERO, 0.0, "Test");
    s.buses.push((BusId::Player, true));
    h.stops.insert(1, s);
    let grant = PassengerGrant {
        id: 7,
        transfer: 1,
        stop: 1,
        spot: 0,
        destination: Some("Destination".into()),
        alternative: None,
        alternative_m: 0.0,
        ride_km: 12.0,
        line_destination: Some("Trip".into()),
        allowed_termini: Some(vec!["Allowed".into()]),
    };
    assert!(h.grant(&grant));
    assert_eq!(
        h.pax(0).unwrap().journey.line,
        None,
        "there is no local array index to import"
    );
    let mut b = bus(cabin());
    b.terminus = Some("Other".into());
    h.buses.last_buses = vec![b.clone()];
    assert_eq!(h.grant_eligible(&grant), Some(false));
    let ix = [(b.id, 0)].into_iter().collect();
    assert_eq!(h.bus_for(0, 1, &[b.clone()], &ix), None);
    b.terminus = Some("Allowed".into());
    h.buses.last_buses = vec![b.clone()];
    assert_eq!(h.grant_eligible(&grant), Some(true));
    assert_eq!(h.bus_for(0, 1, &[b], &ix), Some(BusId::Player));
}

#[test]
fn a_grant_without_a_destination_does_not_draw_one_from_the_clients_timetable() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    h.people.push(f.person(7, State::Standing, true));
    let mut s = stop(DVec3::ZERO, 0.0, "Test");
    s.dests = vec![("Client-only destination".into(), 1.0)];
    s.lines = vec![(
        "Client-only destination".into(),
        ["Client terminus".into()].into_iter().collect(),
    )];
    h.stops.insert(1, s);
    let grant = PassengerGrant {
        id: 7,
        transfer: 1,
        stop: 1,
        spot: 0,
        destination: None,
        alternative: None,
        alternative_m: 0.0,
        ride_km: 12.0,
        line_destination: None,
        allowed_termini: None,
    };
    assert!(h.grant(&grant));
    let p = h.pax(0).unwrap();
    assert_eq!(
        p.journey.dest, None,
        "an unknown host destination stays unknown"
    );
    assert_eq!(p.journey.line, None);
    assert_eq!(p.journey.allowed_termini, None);
    assert_eq!(p.journey.ride_km, 12.0);
}

#[test]
fn a_host_keeps_transfers_until_ack_and_restores_them_on_disconnect() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let remote_bus = BusId::Ai(remote_bus_id(2));
    let mut s = stop(DVec3::ZERO, 0.0, "Test");
    s.buses.push((remote_bus, true));
    s.taken[0] = true;
    h.stops.insert(1, s);
    let mut p = Pax::new(1.1);
    p.task = Task::WaitingForBus;
    p.stop = Some(1);
    p.spot = Some(0);
    p.journey.dest = Some("Destination".into());
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
    let grants = h.hand_over(&[7], remote_bus);
    assert_eq!(grants.len(), 1);
    assert_eq!(h.people.len(), 1);
    assert!(
        h.hand_over(&[7], remote_bus).is_empty(),
        "another claim cannot transfer it again"
    );
    h.finish_handover(7, false);
    assert_eq!(h.pax(0).unwrap().task, Task::WaitingForBus);
    assert_eq!(h.hand_over(&[7], remote_bus).len(), 1);
    h.finish_handover(7, true);
    h.finish_handover(7, true);
    assert!(h.people.is_empty());
    assert!(!h.stops[&1].taken[0]);
}

fn boarder_at_exit_door(f: &Fixture, h: &mut Humans, b: &BusNow, ticket: TicketAction) {
    let mut p = Pax::new(1.1);
    p.task = Task::WalkingToBus;
    p.bus = Some(b.id);
    p.seat = Some(1);
    p.stop = Some(1);
    p.ticket = ticket;
    p.pos = DVec3::new(2.0, 3.0, 0.0);
    let mut s = stop(DVec3::ZERO, 0.0, "Test");
    s.buses.push((b.id, true));
    h.stops.insert(1, s);
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
}

#[test]
fn natural_riders_without_a_purchase_board_at_an_open_exit_but_buyers_keep_the_desk() {
    let f = Fixture::new();
    let b = bus(cabin());
    let ix = [(b.id, 0)].into_iter().collect();
    let exit_door = b.cabin.entries.len() + 1;
    for (natural, ticket, door) in [
        (true, TicketAction::None, exit_door),
        (true, TicketAction::Stamp, exit_door),
        (true, TicketAction::Buy, 0),
        (false, TicketAction::None, 0),
    ] {
        let mut h = Humans::new(&f.root);
        h.set_natural(natural);
        boarder_at_exit_door(&f, &mut h, &b, ticket);
        h.choose_entry(0, std::slice::from_ref(&b), &ix);
        assert_eq!(h.pax(0).unwrap().door, Some(door), "{natural} {ticket:?}");
    }

    let mut h = Humans::new(&f.root);
    boarder_at_exit_door(&f, &mut h, &b, TicketAction::None);
    h.choose_entry(0, std::slice::from_ref(&b), &ix);
    h.pax_mut(0).unwrap().movement = Movement::AtTarget;
    h.task_to_bus(0, 0.05, std::slice::from_ref(&b), &ix, &f.world);
    let p = h.pax(0).unwrap();
    assert_eq!(
        (p.task, p.inside, p.pt),
        (Task::InBusToPlace, Some(b.id), Some(3))
    );
}

#[test]
fn natural_exit_boarders_let_people_off_first() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let b = bus(cabin());
    let ix = [(b.id, 0)].into_iter().collect();
    boarder_at_exit_door(&f, &mut h, &b, TicketAction::None);
    let mut off = Pax::new(1.1);
    off.task = Task::InBusToExit;
    off.inside = Some(b.id);
    off.bus = Some(b.id);
    off.door = Some(1);
    h.people.push(f.person(8, State::Pax(Box::new(off)), false));
    h.task_to_bus(0, 0.05, std::slice::from_ref(&b), &ix, &f.world);
    assert!(h.pax(0).unwrap().short, "waits beside the open exit");
    h.people.pop();
    h.task_to_bus(0, 0.05, std::slice::from_ref(&b), &ix, &f.world);
    assert!(!h.pax(0).unwrap().short);
}

#[test]
fn a_shut_exit_with_an_outside_button_is_requested_then_given_up_for_an_open_door() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut c = cabin();
    Arc::get_mut(&mut c).unwrap().exits[1].button = true;
    let mut b = bus(c);
    b.exit_open = vec![false; 2];
    let ix = [(b.id, 0)].into_iter().collect();
    let exit_door = b.cabin.entries.len() + 1;
    boarder_at_exit_door(&f, &mut h, &b, TicketAction::None);
    h.buses
        .pax_req
        .insert(b.id, (vec![false; exit_door + 1], vec![false; 2]));
    let mut requested = false;
    for _ in 0..4 {
        h.pax_mut(0).unwrap().movement = Movement::ShortOfTarget;
        h.task_to_bus(0, 1.0, std::slice::from_ref(&b), &ix, &f.world);
        assert_eq!(h.pax(0).unwrap().door, Some(exit_door));
        requested |= h.buses.pax_req[&b.id].0[exit_door];
    }
    assert!(requested, "the outside button reaches PAX_Entry<n>_Req");
    for _ in 0..2 {
        h.pax_mut(0).unwrap().movement = Movement::ShortOfTarget;
        h.task_to_bus(0, 1.0, std::slice::from_ref(&b), &ix, &f.world);
    }
    assert_eq!(
        h.pax(0).unwrap().door,
        Some(0),
        "gives up for the open entry"
    );
}

#[test]
fn closing_a_door_at_arrival_never_boards_through_it() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut b = bus(cabin());
    b.entry_open[0] = false;
    let mut p = Pax::new(1.1);
    p.task = Task::WalkingToBus;
    p.bus = Some(b.id);
    p.seat = Some(1);
    p.stop = Some(1);
    p.door = Some(0);
    p.movement = Movement::AtTarget;
    let mut s = stop(DVec3::ZERO, 0.0, "Test");
    s.buses.push((b.id, true));
    h.stops.insert(1, s);
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
    let ix = [(b.id, 0)].into_iter().collect();
    h.task_to_bus(0, 0.05, &[b.clone()], &ix, &f.world);
    assert_eq!(h.pax(0).unwrap().task, Task::WalkingToBus);
    assert_eq!(h.pax(0).unwrap().inside, None);
    b.entry_open[0] = true;
    h.task_to_bus(0, 0.05, &[b.clone()], &ix, &f.world);
    h.pax_mut(0).unwrap().movement = Movement::AtTarget;
    h.task_to_bus(0, 0.05, &[b], &ix, &f.world);
    assert_eq!(h.pax(0).unwrap().inside, Some(BusId::Player));
}

#[test]
fn standing_riders_take_a_free_seat_at_a_stop_without_restarting_their_trip() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut b = bus(cabin());
    let mut p = Pax::new(1.1);
    p.task = Task::SittingInBus;
    p.bus = Some(b.id);
    p.inside = Some(b.id);
    p.seat = Some(0);
    p.pos = DVec3::Y;
    p.journey.km_start = 5.0;
    p.journey.ride_km = 12.0;
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
    h.buses.seats.insert(b.id, vec![true, false, false]);
    b.speed = 5.0;
    assert!(!h.move_to_free_seat(0, &b, true));
    b.speed = 0.0;
    assert!(!h.move_to_free_seat(0, &b, false));
    assert!(h.move_to_free_seat(0, &b, true));
    assert_eq!(
        (
            h.pax(0).unwrap().seat,
            h.pax(0).unwrap().journey.km_start,
            h.pax(0).unwrap().journey.ride_km
        ),
        (Some(1), 5.0, 12.0)
    );
    assert_eq!(h.buses.seats[&b.id], vec![false, true, false]);
    assert!(!h.move_to_free_seat(0, &b, true));
}

#[test]
fn waiting_at_an_exit_keeps_the_bus_floor_on_elevated_maps() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut b = bus(cabin());
    b.pos = DVec3::new(500.0, 735.0, 9.0);
    let ix = [(b.id, 0)].into_iter().collect();
    for state in [Movement::ShortOfPathEnd, Movement::AtPathEnd] {
        let mut p = Pax::new(1.1);
        p.task = Task::InBusToExit;
        p.inside = Some(b.id);
        p.bus = Some(b.id);
        p.pt = Some(3);
        p.pt_target = Some(3);
        p.movement = state;
        p.short = true;
        p.pos = DVec3::new(
            0.0,
            if state == Movement::ShortOfPathEnd {
                2.3
            } else {
                3.0
            },
            0.0,
        );
        h.people.clear();
        h.people.push(f.person(7, State::Pax(Box::new(p)), false));
        h.pax_move(0, 1.0 / 30.0, 1000.0 / 30.0, &f.world, &[b.clone()], &ix);
        assert_eq!(
            h.pax(0).unwrap().pos.z,
            0.0,
            "movement state {state} must use the cabin target, not world origin"
        );
    }
}

#[test]
#[ignore = "requires OMSI_ROOT; writes a cabin audit to target/passenger-cabin-audit.tsv"]
fn audit_installed_bus_cabins() {
    fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, files);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("bus"))
            {
                files.push(path);
            }
        }
    }
    let root = PathBuf::from(std::env::var_os("OMSI_ROOT").expect("OMSI_ROOT"));
    let mut files = Vec::new();
    collect(&root.join("Vehicles"), &mut files);
    files.sort();
    assert!(!files.is_empty());
    let mut report = String::from(
        "vehicle\tplaces\tentries\texits\tunreachable_places\tinvalid_doors\tstatus\n",
    );
    let mut audited = 0;
    for file in &files {
        let def = match ::legacy_vehicle::Vehicle::load(file) {
            Ok(def) => def,
            Err(e) => {
                report.push_str(&format!(
                    "{}\t\t\t\t\t\tdefinition error: {e}\n",
                    file.display()
                ));
                continue;
            }
        };
        if def.passenger_cabin.is_none() {
            report.push_str(&format!(
                "{}\t\t\t\t\t\tno passenger cabin\n",
                file.display()
            ));
            continue;
        }
        match Cabin::load_train(&[(&def, Vec3::ZERO, f32::INFINITY)]) {
            Some(c) => {
                let mut h = Humans::new(&root);
                let mut reachable = 0;
                while h.reserve_place(BusId::Player, &c, None, false).is_some() {
                    reachable += 1;
                }
                let invalid = c
                    .entries
                    .iter()
                    .chain(&c.exits)
                    .filter(|d| d.point.is_none())
                    .count();
                report.push_str(&format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    file.display(),
                    c.seats.len(),
                    c.entries.len(),
                    c.exits.len(),
                    c.seats.len() - reachable,
                    invalid,
                    if c.entries.is_empty() || c.exits.is_empty() {
                        "section-only or unsupported standalone cabin"
                    } else {
                        "audited"
                    }
                ));
                audited += 1;
            }
            None => report.push_str(&format!(
                "{}\t\t\t\t\t\tcabin could not be loaded\n",
                file.display()
            )),
        }
    }
    let output =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/passenger-cabin-audit.tsv");
    std::fs::write(&output, report).unwrap();
    eprintln!(
        "{} bus definitions, {audited} cabins audited: {}",
        files.len(),
        output.display()
    );
    assert!(audited > 0);
}

#[test]
fn left_hand_entries_use_the_cabins_own_side() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut c = cabin();
    let mutable = Arc::get_mut(&mut c).unwrap();
    mutable.entries[0].inside.x = -1.1;
    mutable.entries[0].side = -1.0;
    let b = bus(c);
    let mut p = Pax::new(1.1);
    p.task = Task::WalkingToBus;
    p.bus = Some(b.id);
    p.seat = Some(1);
    p.stop = Some(1);
    p.door = Some(0);
    p.pos = DVec3::new(-2.0, 0.0, 0.0);
    let mut s = stop(DVec3::ZERO, 0.0, "Test");
    s.buses.push((b.id, true));
    h.stops.insert(1, s);
    h.people.push(f.person(7, State::Pax(Box::new(p)), false));
    h.task_to_bus(
        0,
        0.05,
        &[b.clone()],
        &[(b.id, 0)].into_iter().collect(),
        &f.world,
    );
    assert!(h.pax(0).unwrap().clamp_left);
    assert!(h.pax(0).unwrap().clamp_x < 0.0);
}

#[test]
#[ignore = "requires OMSI_ROOT; writes spawn coordinates for real passenger test runs"]
fn installed_maps_provide_passenger_runtime_stops() {
    let root = PathBuf::from(std::env::var_os("OMSI_ROOT").expect("OMSI_ROOT"));
    let mut report = String::from("map\tstop\tname\tx\ty\theading\tz\n");
    for name in ["Grundorf", "HB_76_Bremen-Nord", "TH_Wald"] {
        let dir = root.join("maps").join(name);
        let global = ::map::GlobalCfg::load(&dir.join("global.cfg")).expect(name);
        ::map::configure_grid(&global);
        let mut found = None;
        for t in &global.tiles {
            let tile = ::map::Tile::load(&dir.join(&t.file)).unwrap();
            let terrain =
                ::map::Terrain::load(&crate::scene::tile_companion(&tile.path, ".terrain"))
                    .unwrap();
            for object in &tile.objects {
                if !object.file.to_ascii_lowercase().ends_with("bus_stop.sco") {
                    continue;
                }
                if name == "Grundorf" && object.id != 108 {
                    continue;
                }
                // Match the placed object's world heading, not the map record's rotation convention.
                let xf = ::geometry::object_rotation(::geometry::map_rotation(object.rot));
                let forward = xf.transform_vector3(Vec3::Y);
                let heading = forward.x.atan2(forward.y).to_degrees();
                let (x, y) = ::map::tile_local_to_world(t.x, t.y, object.pos[0], object.pos[1]);
                let z = terrain.sample(object.pos[0] as f32, object.pos[1] as f32) as f64
                    + object.pos[2];
                found = Some((
                    object.id,
                    object.extra.first().cloned().unwrap_or_default(),
                    x,
                    y,
                    heading,
                    z,
                ));
                break;
            }
            if found.is_some() {
                break;
            }
        }
        let (id, label, x, y, heading, z) = found.expect("a real bus stop");
        report.push_str(&format!(
            "{name}\t{id}\t{label}\t{x}\t{y}\t{heading}\t{z}\n"
        ));
    }
    std::fs::write(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/passenger-runtime-stops.tsv"),
        report,
    )
    .unwrap();
}

#[test]
fn debarking_passengers_disperse_along_sidewalk_network() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let lane = ::traffic::LaneBuilder::polyline(
        vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 50.0, 0.0)],
        ::traffic::LaneKind::Sidewalk,
        2.0,
    );
    let net = ::traffic::Network {
        lanes: vec![lane],
        ..Default::default()
    };
    h.walking.ped = Some(PedNet::build(&net));
    h.people.push(f.person(1, State::Standing, false));
    let mut s = stop(DVec3::new(0.0, 20.0, 0.0), 0.0, "TestStop");
    s.lane = Some((0, 20.0));
    h.stops.insert(1, s);

    h.walk_street(0, DVec3::new(0.0, 20.0, 0.0), 0.0, Some(1), Some(&net));
    match &h.people[0].state {
        State::Strolling(walk) => {
            assert_eq!(walk.legs.len(), 1);
            let leg = walk.legs[0];
            assert_eq!(leg.lane, 0);
            assert_eq!(leg.a, 20.0);
            assert!(
                leg.b == 0.0 || (leg.b - 50.0).abs() < 1e-3,
                "leg.b must be end node 0 or len, got {}",
                leg.b
            );
            assert!(
                leg.len() > 10.0,
                "leg length must be > 10m, got {}",
                leg.len()
            );
        }
        other => panic!("expected Strolling, got {:?}", other),
    }
}

#[test]
fn ik_toggle_configures_procedural_sitting_and_standing() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    h.set_ik(true);
    assert!(h.ik);
    let b = bus(cabin());
    let mut p = Pax::new(1.1);
    p.inside = Some(b.id);
    p.bus = Some(b.id);
    p.seat = Some(1);
    h.people.push(f.person(1, State::Pax(Box::new(p)), false));
    let ix = [(b.id, 0)].into_iter().collect();

    h.set_task(0, Task::SittingInBus, &[b.clone()], &ix, &f.world);
    assert!(h.pax(0).unwrap().seat_approach.is_some());
    for _ in 0..100 {
        h.pax_frame(
            0.05,
            &f.world,
            None,
            std::slice::from_ref(&b),
            &ix,
            &HashMap::new(),
            None,
            &mut |_, _, _, _| panic!("no payment"),
            &mut false,
            &mut vec![],
        );
        h.animate(0.05, &f.world, std::slice::from_ref(&b), &ix);
    }
    assert_eq!(h.people[0].activity, Activity::Sit);
    let pax = h.pax(0).unwrap();
    assert_eq!(pax.posture, Posture::Sitting);

    h.set_task(0, Task::InBusToExit, &[b], &ix, &f.world);
    assert_eq!(h.people[0].activity, Activity::Stand);
}

#[test]
fn bus_aisle_jam_squeeze_unblocks_frozen_passengers() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let b = bus(cabin());
    let mut p1 = Pax::new(1.1);
    p1.inside = Some(b.id);
    p1.bus = Some(b.id);
    p1.movement = Movement::AlongPath;
    p1.pt = Some(1);
    p1.pt_target = Some(3);
    p1.pos = DVec3::ZERO;
    p1.yaw = 0.0;
    h.people.push(f.person(1, State::Pax(Box::new(p1)), false));

    let mut p2 = Pax::new(1.1);
    p2.inside = Some(b.id);
    p2.bus = Some(b.id);
    p2.movement = Movement::AlongPath;
    p2.pt = Some(2);
    p2.pt_target = Some(3);
    p2.pos = DVec3::new(0.0, 0.4, 0.0);
    p2.yaw = 0.0;
    h.people.push(f.person(2, State::Pax(Box::new(p2)), false));

    let ix = [(b.id, 0)].into_iter().collect();

    // Verify p2 blocks p1 (same direction, within 0.6m -> block = 3)
    let (block, _, _) = h.pax_blockers(0, &[b.clone()], &ix);
    assert!(
        block >= Obstruction::Facing,
        "expected block >= 2, got {}",
        block
    );

    // Simulate jam accumulating up to 0.75s
    h.pax_mut(0).unwrap().jam = 0.75;
    // Next tick with dt = 0.1s exceeds 0.8s threshold
    h.pax_move(0, 0.1, 100.0, &f.world, &[b], &ix);
    let p = h.pax(0).unwrap();
    assert_eq!(p.jam, 0.0);
    assert_eq!(p.squeeze, 1.5);
    assert_eq!(p.obstruction, Obstruction::Clear);
}

#[test]
#[ignore = "requires OMSI_ROOT; exports the actual driver door bindings for runtime tests"]
fn installed_buses_provide_passenger_runtime_controls() {
    let root = PathBuf::from(std::env::var_os("OMSI_ROOT").expect("OMSI_ROOT"));
    let buses = [
        (
            "man-nlc-12c-zf",
            "Vehicles/MAN_NewLionsCity/MAN_12C_3door_ZF.bus",
        ),
        ("en92", "Vehicles/MAN_NL_NG/MAN_EN92_main_EUR.bus"),
        ("gn92", "Vehicles/MAN_NL_NG/MAN_GN92_main_EUR.bus"),
        ("bremen-nl263", "Vehicles/HB76_MAN_LionsCity/MAN_NL263.bus"),
        (
            "bremen-ng313",
            "Vehicles/HB76_MAN_LionsCity/MAN_NG313_main.bus",
        ),
        (
            "th-setra",
            "Vehicles/TH_Ueberlandbus/S315UL-GT_Euro3_Automatik.bus",
        ),
    ];
    let mut report = String::from("id\tbus\ttriggers\treleases\n");
    for (id, bus) in buses {
        let ty = Arc::new(::simulation::VehicleType::load_ai(&root, &root.join(bus)).unwrap());
        let groups = crate::player::door_keys(&ty);
        let mut vehicle = VehicleInstance::new(
            ty,
            ::simulation::VehicleHost::new(::simulation::SimClock::default()),
        );
        let mut triggers = Vec::new();
        for group in groups {
            for trigger in crate::player::door_group_to_fire(&mut vehicle, &group) {
                if !triggers.contains(&trigger) {
                    triggers.push(trigger);
                }
            }
        }
        assert!(!triggers.is_empty(), "{bus}");
        let releases = triggers
            .iter()
            .map(|n| format!("{n}_off"))
            .filter(|n| vehicle.ty.program.trigger(n).is_some())
            .collect::<Vec<_>>();
        report.push_str(&format!(
            "{id}\t{bus}\t{}\t{}\n",
            triggers.join(","),
            releases.join(",")
        ));
    }
    std::fs::write(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/passenger-runtime-controls.tsv"),
        report,
    )
    .unwrap();
}

#[test]
fn the_driver_getting_up_sits_at_the_wheel_first_and_then_stands_up() {
    let f = Fixture::new();
    let driver = f.human_at("Humans/Test/Driver.hum", None);
    let other = f.human_at("Humans/Test/Other.hum", None);
    let mut h = Humans::new(&f.root);
    h.types.clear();
    h.population.clear();
    h.alternates.clear();
    h.types.push(other);
    h.population.push(0);
    h.map_humans_done = true;
    let renderer = noop_renderer();
    let mut scene = renderer.new_scene();
    let b = bus(cabin());
    let ix = [(b.id, 0)].into_iter().collect();
    let kind = h.avatar_figure(driver.clone());
    let cmd = |wheel| AvatarCmd {
        pos: DVec3::ZERO,
        heading: 0.0,
        vel: DVec2::ZERO,
        lift: 0.0,
        seat: None,
        floor: None,
        aboard: Some((BusId::Player, Vec3::new(0.5, 0.0, 0.0))),
        wheel,
    };
    let seat = Some((BusId::Player, Vec3::new(0.0, 0.0, 0.5), 0.0, 0.45));
    h.avatar(1, &f.world, &renderer, &mut scene, cmd(seat), kind);
    assert!(
        Arc::ptr_eq(&h.people[0].ty, &driver),
        "the one who drove gets up"
    );
    h.animate_avatar(0, 1.0 / 30.0, &f.world, std::slice::from_ref(&b), &ix);
    assert!(
        h.people[0].pose.sit_amount() > 0.95,
        "sitting at the wheel when first seen, not sitting down"
    );
    h.avatar(1, &f.world, &renderer, &mut scene, cmd(None), kind);
    for _ in 0..75 {
        h.animate_avatar(0, 1.0 / 30.0, &f.world, std::slice::from_ref(&b), &ix);
    }
    assert!(h.people[0].pose.sit_amount() < 0.05, "stood up");
}

#[test]
fn the_validator_is_used_where_the_way_in_passes_it() {
    let f = Fixture::new();
    let mut h = Humans::new(&f.root);
    let mut c = cabin();
    Arc::get_mut(&mut c).unwrap().stamper = Some((Some(3), Vec3::new(0.35, 1.0, 1.2)));
    let b = bus(c);
    let ix: HashMap<BusId, usize> = [(b.id, 0)].into_iter().collect();
    let mut p = Pax::new(1.1);
    p.task = Task::InBusToPlace;
    p.inside = Some(b.id);
    p.bus = Some(b.id);
    p.seat = Some(1);
    p.ticket = TicketAction::Stamp;
    p.movement = Movement::AlongPath;
    p.pos = DVec3::Y;
    p.pt = Some(2);
    p.pt_target = Some(3);
    h.people.push(f.person(42, State::Pax(Box::new(p)), false));
    h.task_to_place(
        0,
        std::slice::from_ref(&b),
        &ix,
        &f.world,
        None,
        &mut |_, _, _, _| {},
        &mut false,
    );
    let p = h.pax(0).unwrap();
    assert_eq!(p.fare_phase, FarePhase::Validating);
    assert_ne!(p.movement, Movement::AlongPath);
    assert!(p.target.y < 1.5, "stamps at {:?}", p.target);
}

#[test]
fn without_rear_entry_everybody_boards_at_the_entries() {
    let f = Fixture::new();
    for rear in [true, false] {
        let mut h = Humans::new(&f.root);
        h.set_natural(true);
        h.rear_entry = rear;
        let mut b = bus(cabin());
        b.entry_open = vec![true];
        b.exit_open = vec![true, true];
        let ix: HashMap<BusId, usize> = [(b.id, 0)].into_iter().collect();
        let mut p = Pax::new(1.1);
        p.task = Task::ToBus;
        p.bus = Some(b.id);
        p.seat = Some(1);
        p.pos = DVec3::new(1.8, 3.0, 0.0);
        h.people.push(f.person(42, State::Pax(Box::new(p)), false));
        h.choose_entry(0, std::slice::from_ref(&b), &ix);
        let door = h.pax(0).unwrap().door.unwrap();
        if rear {
            assert!(door >= b.cabin.entries.len(), "door {door}");
        } else {
            assert!(door < b.cabin.entries.len(), "door {door}");
        }
    }
}
