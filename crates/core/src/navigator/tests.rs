use super::*;
use ::simulation::traffic::Lane;

fn straight(a: (f64, f64), b: (f64, f64)) -> Lane {
    ::simulation::traffic::LaneBuilder::polyline(
        vec![DVec3::new(a.0, a.1, 0.0), DVec3::new(b.0, b.1, 0.0)],
        LaneKind::Street,
        3.0,
    )
}

#[test]
fn a_way_back_joins_the_route_ahead() {
    let lanes = vec![
        straight((0.0, 0.0), (0.0, 100.0)),
        straight((0.0, 100.0), (100.0, 100.0)),
        straight((100.0, 100.0), (200.0, 100.0)),
        straight((0.0, 100.0), (-100.0, 100.0)),
        straight((-100.0, 100.0), (-100.0, 200.0)),
        straight((-100.0, 200.0), (100.0, 200.0)),
        straight((100.0, 200.0), (100.0, 100.0)),
    ];
    let mut net = Network {
        lanes,
        ..Default::default()
    };
    net.link(1.5);
    for (a, n) in [
        (0, vec![1, 3]),
        (1, vec![2]),
        (3, vec![4]),
        (4, vec![5]),
        (5, vec![6]),
        (6, vec![2]),
    ] {
        net.lanes[a].next = n;
    }
    let route = [0usize, 1, 2];
    let (path, join) = way_back(
        &net,
        DVec3::new(-40.0, 100.0, 0.0),
        270.0,
        &route[1..],
        6000.0,
    )
        .expect("a way");
    assert_eq!(path.first(), Some(&3));
    assert!(path.contains(&6), "{path:?}");
    assert_eq!(route[1..][join], 2, "{path:?} join {join}");
    assert!(heading_vec(90.0).x > 0.99);
}

#[test]
fn a_way_back_changes_lanes_where_the_splines_do_not_link_them() {
    let lanes = vec![
        straight((0.0, 0.0), (0.0, 100.0)),
        straight((0.0, 100.0), (0.0, 200.0)),
        straight((3.5, 0.0), (3.5, 100.0)),
        straight((3.5, 100.0), (3.5, 200.0)),
        straight((3.5, 200.0), (3.5, 300.0)),
    ];
    let mut net = Network {
        lanes,
        ..Default::default()
    };
    net.link(1.5);
    for (a, n) in [(0, vec![1]), (1, vec![]), (2, vec![3]), (3, vec![4])] {
        net.lanes[a].next = n;
    }
    let route = [4usize];
    let (path, join) = way_back(&net, DVec3::new(0.0, 10.0, 0.0), 0.0, &route, 6000.0)
        .expect("a way over to the lane beside");
    assert_eq!(path, vec![0, 3]);
    assert_eq!(join, 0);
    // the line drifts across instead of stepping
    for l in net.lanes.iter_mut() {
        *l = ::simulation::traffic::LaneBuilder::polyline(
            (0..=50)
                .map(|k| l.points[0].lerp(l.points[1], k as f64 / 50.0))
                .collect(),
            LaneKind::Street,
            3.0,
        );
    }
    let lines = route_lines(&net, &[0, 3, 4], 0.0, f64::MAX);
    assert_eq!(lines.len(), 1);
    let x_at = |y: f64| {
        lines[0]
            .windows(2)
            .find(|w| w[0].0.y <= y && w[1].0.y >= y)
            .map(|w| w[0].0.x + (w[1].0.x - w[0].0.x) * (y - w[0].0.y) / (w[1].0.y - w[0].0.y))
            .unwrap()
    };
    assert!(x_at(102.0) < 1.0 && x_at(115.0) > 1.0 && x_at(115.0) < 2.5, "{lines:?}");
    assert!((x_at(140.0) - 3.5).abs() < 0.01);
}

#[test]
fn projection_puts_the_look_at_point_in_the_middle() {
    let view =
        glam::camera::rh::view::look_at_mat4(Vec3::new(0.0, -50.0, 80.0), Vec3::ZERO, Vec3::Z);
    let l = Layer::world(view, 0.7, [0.0, 0.0, 400.0, 300.0], [0.0; 4], 0.0, 1.0);
    let p = project(l.view_proj, [0.0, 0.0, 400.0, 300.0], Vec3::ZERO).unwrap();
    assert!((p - Vec2::new(200.0, 150.0)).length() < 0.5, "{p}");
    let ahead = project(
        l.view_proj,
        [0.0, 0.0, 400.0, 300.0],
        Vec3::new(0.0, 30.0, 0.0),
    )
        .unwrap();
    assert!(ahead.y < 150.0, "ahead is up the picture: {ahead}");
}

#[test]
fn editor_only_lanes_do_not_become_gps_roads() {
    let visible = straight((0.0, 0.0), (100.0, 0.0));
    let mut hidden = straight((0.0, 30.0), (100.0, 30.0));
    hidden.invisible = true;
    let net = Network {
        lanes: vec![visible.clone(), hidden],
        ..Default::default()
    };
    let drawn = visible_road_lanes(&net);
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].1.points, visible.points);
}

#[test]
fn separate_asphalt_meshes_corroborate_invisible_traffic_splines() {
    let mut on_road = straight((0.0, 0.0), (100.0, 0.0));
    on_road.invisible = true;
    let mut helper = straight((0.0, 25.0), (100.0, 25.0));
    helper.invisible = true;
    let mut bridge = straight((0.0, 0.0), (100.0, 0.0));
    bridge.points.iter_mut().for_each(|p| p.z = 8.0);
    bridge.invisible = true;
    let mut net = Network {
        lanes: vec![on_road, helper, bridge],
        ..Default::default()
    };
    let surface = (vec![DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0)], 8.0);
    confirm_road_surfaces(&mut net, &[surface.clone()]);
    assert!(!net.lanes[0].invisible);
    assert!(net.lanes[1].invisible);
    assert!(
        net.lanes[2].invisible,
        "asphalt below a bridge must not corroborate its paths"
    );
    assert_eq!(
        road_geometry(&net).len(),
        1,
        "surface evidence must not draw a second road"
    );
}

#[test]
fn adjacent_spline_lanes_form_carriageways_without_filling_the_median() {
    let mut lanes = Vec::new();
    for (path, offset) in [-10.0f32, -7.0, 7.0, 10.0].into_iter().enumerate() {
        let mut lane = straight((offset as f64, 0.0), (offset as f64, 100.0));
        lane.source = 1;
        lane.key = Some(LaneKey {
            tile: (0, 0),
            id: 42,
            path: path as u16,
        });
        lane.offset = offset;
        if offset < 0.0 {
            lane.points.reverse();
            lane.reversed = true;
        }
        lanes.push(lane);
    }
    let net = Network {
        lanes,
        ..Default::default()
    };
    let roads = road_geometry(&net);
    assert_eq!(roads.len(), 2);
    assert_eq!(roads[0].width, 6.0);
    assert_eq!(roads[1].width, 6.0);
    assert_eq!(
        roads[0].points,
        vec![DVec3::new(-8.5, 0.0, 0.0), DVec3::new(-8.5, 100.0, 0.0)]
    );
    assert_eq!(roads[1].points[0].x, 8.5);
    assert_eq!(net.lanes.len(), 4);
    assert_eq!(net.lanes[0].points[0].y, 100.0);
}

#[test]
fn opposite_directions_draw_once_even_when_reverse_is_first() {
    let mut a = straight((0.0, 0.0), (0.0, 100.0));
    a.key = Some(LaneKey {
        tile: (0, 0),
        id: 42,
        path: 0,
    });
    let mut b = a.clone();
    b.points.reverse();
    b.reversed = true;
    let net = Network {
        lanes: vec![b, a],
        ..Default::default()
    };
    assert_eq!(visible_road_lanes(&net).len(), 1);
}

#[test]
fn junction_helpers_between_roads_remain_connected() {
    let mut lanes = vec![
        straight((0.0, 0.0), (0.0, 40.0)),
        straight((0.0, 40.0), (0.0, 50.0)),
        straight((0.0, 50.0), (0.0, 60.0)),
        straight((0.0, 60.0), (0.0, 100.0)),
        straight((30.0, 0.0), (30.0, 100.0)),
    ];
    lanes[0].next = vec![1];
    lanes[1].next = vec![2];
    lanes[2].next = vec![3];
    for i in [1, 2, 4] {
        lanes[i].invisible = true;
    }
    let mut net = Network {
        lanes,
        ..Default::default()
    };
    confirm_road_surfaces(&mut net, &[]);
    assert!(!net.lanes[1].invisible && !net.lanes[2].invisible);
    assert!(net.lanes[4].invisible);
    assert_eq!(road_geometry(&net).len(), 4);
}

#[test]
fn placement_gaps_are_bridged_only_where_the_graph_connects() {
    let mut lanes = vec![
        straight((0.0, 0.0), (0.0, 40.0)),
        straight((0.0, 41.0), (0.0, 80.0)),
        straight((2.0, 41.0), (2.0, 80.0)),
    ];
    lanes[0].next = vec![1];
    let roads = road_geometry(&Network {
        lanes,
        ..Default::default()
    });
    assert_eq!(roads.len(), 4);
    assert_eq!(
        roads[3].points,
        vec![DVec3::new(0.0, 40.0, 0.0), DVec3::new(0.0, 41.0, 0.0)]
    );
}

#[test]
fn paved_areas_without_driving_paths_do_not_draw_streets() {
    let mut net = Network::default();
    confirm_road_surfaces(
        &mut net,
        &[(vec![DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0)], 20.0)],
    );
    assert!(road_geometry(&net).is_empty());
}

#[test]
fn stacked_paths_on_one_spline_are_not_merged() {
    let mut a = straight((0.0, 0.0), (0.0, 100.0));
    a.source = 1;
    a.key = Some(LaneKey {
        tile: (0, 0),
        id: 42,
        path: 0,
    });
    let mut b = a.clone();
    b.key.as_mut().unwrap().path = 1;
    b.points.iter_mut().for_each(|p| p.z = 8.0);
    let roads = road_geometry(&Network {
        lanes: vec![a, b],
        ..Default::default()
    });
    assert_eq!(roads.len(), 2);
    assert_eq!(roads[0].points[0].z, 0.0);
    assert_eq!(roads[1].points[0].z, 8.0);
}

#[test]
fn crowded_stop_markers_and_labels_do_not_overlap() {
    let p = Vec2::new(200.0, 200.0);
    let markers = spaced_markers(
        [(0, p), (1, p + Vec2::X * 5.0), (2, p + Vec2::X * 35.0)],
        20.0,
    );
    assert_eq!(markers.iter().map(|m| m.0).collect::<Vec<_>>(), vec![0, 2]);
    let win = Rect::new(0.0, 0.0, 600.0, 400.0);
    let first = stop_label_rect(p, 150.0, 1.0, win, &[]).unwrap();
    let second = stop_label_rect(p + Vec2::X * 35.0, 150.0, 1.0, win, &[first]).unwrap();
    assert!(!rects_overlap(&first, &second));
    let edge = stop_label_rect(Vec2::new(590.0, 200.0), 150.0, 1.0, win, &[]).unwrap();
    assert!(edge.right() < 600.0);
    assert!(stop_label_rect(p, 150.0, 1.0, win, &[win]).is_none());
}

#[test]
fn object_lanes_side_by_side_widen_into_one_carriageway() {
    // two lanes of a road object 3.5 m apart, 2.6 m wide each: drawn as wide as their
    // spacing they meet, a lane 9 m away (beyond a median) stays apart
    let mut lanes = vec![
        straight((0.0, 0.0), (0.0, 100.0)),
        straight((3.5, 0.0), (3.5, 100.0)),
        straight((12.5, 0.0), (12.5, 100.0)),
    ];
    for l in lanes.iter_mut() {
        l.width = 2.6;
        l.source = 2;
    }
    let mut net = Network {
        lanes,
        ..Default::default()
    };
    net.link(1.5);
    net.build_grid();
    let roads = road_geometry(&net);
    let width_at = |x: f64| {
        roads
            .iter()
            .find(|r| (r.points[0].x - x).abs() < 0.01)
            .map(|r| r.width)
            .unwrap()
    };
    assert!(width_at(0.0) >= 3.5 && width_at(3.5) >= 3.5, "{roads:?}");
    assert!(width_at(12.5) < 3.0, "a lane beyond the median keeps its own width");
}

#[test]
fn the_graph_draws_a_spline_once_and_joins_a_short_gap() {
    // a spline whose carriageway the map gives, and a road object starting 2 m past its end
    let mut a = straight((0.0, 0.0), (0.0, 100.0));
    a.source = 1;
    a.key = Some(LaneKey {
        tile: (0, 0),
        id: 7,
        path: 0,
    });
    let mut b = straight((0.0, 104.5), (0.0, 200.0));
    b.source = 2;
    let mut net = Network {
        lanes: vec![a, b],
        ..Default::default()
    };
    net.link(1.5);
    net.build_grid();
    let ways = vec![crate::scene::NavCarriageway {
        points: vec![DVec3::ZERO, DVec3::new(0.0, 100.0, 0.0)],
        width: 6.0,
        tile: (0, 0),
        id: 7,
        cars: true,
    }];
    let g = RoadGraph::build(&net, &ways);
    let along_spline = g
        .roads
        .iter()
        .filter(|r| r.spline == Some(((0, 0), 7)))
        .count();
    assert_eq!(along_spline, 1, "the spline's lanes are not drawn beside its carriageway");
    assert!(
        g.roads.iter().any(|r| r.spline.is_none()
            && r.points.first().is_some_and(|p| (p.y - 100.0).abs() < 0.01)),
        "the gap after the carriageway is joined: {:?}",
        g.roads.iter().map(|r| (r.points.first(), r.points.last())).collect::<Vec<_>>()
    );
    assert!(g.open_ends.is_empty());
    let (near, _) = g.near(DVec2::new(0.0, 50.0), 20.0);
    assert!(!near.is_empty());
}

#[test]
fn a_junction_object_gets_its_surface() {
    let mut lanes = vec![
        straight((-10.0, 0.0), (10.0, 0.0)),
        straight((10.0, 2.0), (-10.0, 2.0)),
        straight((0.0, -10.0), (0.0, 0.0)),
    ];
    for l in lanes.iter_mut() {
        l.source = 2;
        l.key = Some(LaneKey {
            tile: (0, 0),
            id: 99,
            path: 0,
        });
    }
    let net = Network {
        lanes,
        ..Default::default()
    };
    let g = RoadGraph::build(&net, &[]);
    assert_eq!(g.areas.len(), 1);
    // a ring of paths (a roundabout): its island stays dark
    let ring: Vec<_> = (0..8)
        .map(|k| {
            let a = |k: usize| (k as f64 * std::f64::consts::FRAC_PI_4).sin_cos();
            let ((s0, c0), (s1, c1)) = (a(k), a(k + 1));
            let mut l = straight((15.0 * s0, 15.0 * c0), (15.0 * s1, 15.0 * c1));
            l.source = 2;
            l.key = Some(LaneKey {
                tile: (0, 0),
                id: 100,
                path: k as u16,
            });
            l
        })
        .collect();
    let ring = Network {
        lanes: ring,
        ..Default::default()
    };
    assert!(RoadGraph::build(&ring, &[]).areas.is_empty());
    let hull = &g.areas[0];
    assert!(hull.iter().any(|p| p.y < -9.0) && hull.iter().any(|p| p.y > 2.5));
}

#[test]
fn the_route_line_is_one_smooth_line_and_breaks_where_lanes_jump() {
    // a lane change: two parallel lanes 3.5 m apart, the route steps from one to the other
    let lanes = vec![
        straight((0.0, 0.0), (0.0, 100.0)),
        straight((3.5, 100.0), (3.5, 200.0)),
        straight((500.0, 0.0), (500.0, 100.0)),
    ];
    let mut net = Network {
        lanes,
        ..Default::default()
    };
    for l in net.lanes.iter_mut() {
        *l = ::simulation::traffic::LaneBuilder::polyline(
            (0..=50)
                .map(|k| l.points[0].lerp(l.points[1], k as f64 / 50.0))
                .collect(),
            LaneKind::Street,
            3.0,
        );
    }
    let lines = route_lines(&net, &[0, 1, 2], 1000.0, f64::MAX);
    assert_eq!(lines.len(), 2, "the jump to the third lane starts a new line");
    let first = &lines[0];
    assert!((first[0].1 - 1000.0).abs() < 1e-6);
    assert!(first.windows(2).all(|w| w[1].1 >= w[0].1), "distance along grows");
    // no step: neighbouring points never more than a few metres apart sideways
    for w in first.windows(2) {
        assert!((w[1].0.x - w[0].0.x).abs() < 1.0, "{:?} -> {:?}", w[0].0, w[1].0);
    }
    assert_eq!(first[0].0, net.lanes[0].start(), "the line starts where the route does");
}

#[test]
fn a_convex_hull_keeps_the_outline() {
    let pts = vec![
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(10.0, 0.0, 0.0),
        DVec3::new(10.0, 10.0, 0.0),
        DVec3::new(0.0, 10.0, 0.0),
        DVec3::new(5.0, 5.0, 0.0),
    ];
    let h = convex_hull(pts);
    assert_eq!(h.len(), 4);
}
