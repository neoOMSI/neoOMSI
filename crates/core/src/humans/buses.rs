use super::*;

/// Associate seated places with a cabin floor path; actual foot-root alignment uses
/// each human's measured rig rather than this shared lookup offset.
pub(super) const SEAT_FRONT: f32 = 0.34;

/// A bus as the passengers know it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BusId {
    Player,
    Ai(u64),
}

impl BusId {
    /// Floor space of the bus (0 is the ground).
    pub fn space(self) -> u64 {
        match self {
            BusId::Player => 1,
            BusId::Ai(id) => 2 + id,
        }
    }
}

/// An `[entry]` or `[exit]` of a cabin, in the bus frame.
#[derive(Debug, Clone)]
pub(super) struct Door {
    /// The door's path point (the threshold) and its index.
    pub(super) inside: Vec3,
    pub(super) point: Option<usize>,
    /// Where somebody stands just outside, at ground level.
    pub(super) outside: Vec3,
    /// +1 on the right side of the bus, -1 on the left.
    pub(super) side: f32,
    /// Direction along the bus (+1 forwards) in which the queue at this door runs.
    pub(super) queue_dir: f32,
    /// A passenger who still has to buy a ticket may board here (no `{noticketsale}`).
    pub(super) sells: bool,
    /// `{withbutton}`: a door the passenger opens with the request button, worth walking to
    /// while it is still shut.
    pub(super) button: bool,
    /// Where people getting off wait for the door to open: the path point next to it.
    pub(super) wait: Vec3,
}

#[derive(Debug, Clone)]
pub(super) struct Seat {
    /// Path point serving this place, on its own floor; None when its cabin is invalid.
    pub(super) point: Option<usize>,
    /// The `[passpos]` point: a seated passenger's hip, a standing one's feet.
    pub(super) pos: Vec3,
    /// The floor in front of it, where the feet go (and where a seated passenger stands
    /// before sitting down and after getting up).
    pub(super) floor: Vec3,
    pub(super) rot: f32,
    pub(super) seated: bool,
    /// The `[passpos]`'s seat height (+0x20; 0: a standing place).
    pub(super) height: f32,
    /// Its number for the scripts (`GetHumanCountOnSeat`): Omsi.exe's place in the file
    /// among the `[passpos]` and `[drivpos]`, the sections behind counted on after those
    /// in front (0x7d39a4 asks the next one for a number past its own places).
    pub(super) omsi_seat: usize,
}

/// Cabins list some `[passpos]` twice on one spot (the Citaro C2's).
fn same_spot(a: &Seat, b: &Seat) -> bool {
    let (p, q) = match (a.seated, b.seated) {
        (true, true) => (a.pos, b.pos),
        (false, false) => (a.pos, b.pos),
        (true, false) => (a.floor, b.pos),
        (false, true) => (a.pos, b.floor),
    };
    (p - q).truncate().length() < 0.3 && (p.z - q.z).abs() < 0.3
}

pub(super) fn place_taken(seats: &[Seat], taken: &[bool], k: usize) -> bool {
    let busy = |j: usize| taken.get(j).copied().unwrap_or(false);
    busy(k)
        || seats
            .iter()
            .enumerate()
            .any(|(j, s)| j != k && busy(j) && same_spot(&seats[k], s))
}

/// The room at the knees (m), and the side (+1 right) the aisle is on.
pub(super) fn legroom(seats: &[Seat], k: usize) -> Option<(f32, f32)> {
    let s = seats.get(k).filter(|s| s.seated)?;
    let r = s.rot.to_radians();
    let (fwd, right) = (
        glam::Vec2::new(r.sin(), r.cos()),
        glam::Vec2::new(r.cos(), -r.sin()),
    );
    let room = seats
        .iter()
        .enumerate()
        .filter(|(j, o)| *j != k && o.seated && (o.pos.z - s.pos.z).abs() < 0.4)
        .filter_map(|(_, o)| {
            let d = (o.pos - s.pos).truncate();
            if d.dot(fwd) < 0.2 || d.dot(right).abs() > 0.3 {
                return None;
            }
            let turn = (o.rot - s.rot).rem_euclid(360.0);
            let turn = turn.min(360.0 - turn);
            if turn < 45.0 {
                Some(d.dot(fwd) - BACKREST)
            } else if turn > 135.0 {
                Some(d.dot(fwd) * 0.5)
            } else {
                None
            }
        })
        .fold(f32::INFINITY, f32::min);
    let aisle = if (s.pos.x * r.cos()).abs() < 0.05 {
        1.0
    } else {
        -(s.pos.x * r.cos()).signum()
    };
    room.is_finite().then_some((room, aisle))
}

const BACKREST: f32 = 0.2;

pub(super) fn prefer_seated_places(free: &mut Vec<usize>, seats: &[Seat], prefer_seats: bool) {
    if prefer_seats && free.iter().any(|&k| seats[k].seated) {
        free.retain(|&k| seats[k].seated);
    }
}

/// What passengers need to know about one vehicle type's cabin.
pub(super) struct Cabin {
    pub(super) data: PassengerCabin,
    pub(super) graph: PathGraph,
    pub(super) links: Vec<(i32, i32, bool)>,
    /// Each link's footstep sounds: its section's `[stepsoundpack]` named by the link's
    /// `[next_stepsound]` (index into `step_packs`), none where the paths.cfg gives none -
    /// Omsi.exe hears no steps there - and on the joint between two sections.
    pub(super) link_pack: Vec<Option<usize>>,
    pub(super) step_packs: Vec<Arc<[String]>>,
    pub(super) entries: Vec<Door>,
    pub(super) exits: Vec<Door>,
    /// Where a passenger stands at the cash desk, its path point, and the heading (bus
    /// frame) they face: between the desk top, where the money goes, and the driver.
    pub(super) desk: Option<(Vec3, Option<usize>, f64)>,
    pub(super) seats: Vec<Seat>,
    /// The sections (one for a rigid bus), front first; everything above is in the
    /// unfolded frame of the front section.
    pub(super) parts: Vec<CabinPart>,
    /// Each link's room height (`[next_roomheight]`; 2 m before any).
    pub(super) link_room: Vec<f32>,
    /// The routing tables of the path network (sub_72410c).
    pub(super) routes: Vec<Vec<RouteLink>>,
    /// The validator and the cash desk as Omsi.exe keeps them - one each, the last of the
    /// file (cabin +0x14/+0x18, +0x28/+0x2c): (path point, device).
    pub(super) stamper: Option<(Option<usize>, Vec3)>,
    pub(super) sale: Option<(Option<usize>, Vec3)>,
    /// Where the money goes (+0x38) and where the change is taken from (+0x58), with the
    /// money point's spread.
    pub(super) money_point: Option<Vec3>,
    pub(super) money_var: Option<(Vec3, [f32; 2])>,
    pub(super) change_point: Option<Vec3>,
}

/// The people on each seat by the scripts' numbers (`Seat::omsi_seat`, the `[drivpos]`
/// counted with the `[passpos]`), from the places (indices into `seats`) taken by people
/// sitting there. (Counted by the `[passpos]` alone, every seat of a cabin with the
/// driver's place first was one off: a tip-up seat folded down under the next one.)
pub(super) fn seat_numbers(seats: &[Seat], sitting: impl Iterator<Item = usize>) -> Vec<u32> {
    let n = seats.iter().map(|s| s.omsi_seat + 1).max().unwrap_or(0);
    let mut out = vec![0u32; n];
    for k in sitting {
        if let Some(c) = seats.get(k).and_then(|s| out.get_mut(s.omsi_seat)) {
            *c += 1;
        }
    }
    out
}

/// A section of an articulated bus in its cabin's unfolded frame.
#[derive(Debug, Clone, Copy)]
pub(super) struct CabinPart {
    /// Where the section's own origin lies.
    pub(super) offset: Vec3,
    /// The unfolded y of the joint in front of it (the front section: none, +inf).
    pub(super) joint_y: f32,
}

/// One vehicle of a coupled train as a cabin is put together from it: its definition, its
/// origin in the front vehicle's unfolded frame, and the unfolded y of its front joint.
pub(super) type TrainPart<'a> = (&'a ::legacy_vehicle::Vehicle, Vec3, f32);

/// The sections of `v` passengers can walk through, front first: the vehicle and every
/// coupled part straight behind it (a part coupled the wrong way round and all behind it
/// are left out).
pub(super) fn train_parts(v: &VehicleInstance) -> Vec<TrainPart<'_>> {
    let mut out: Vec<TrainPart<'_>> = vec![(&v.ty.def, Vec3::ZERO, f32::INFINITY)];
    let mut offset = Vec3::ZERO;
    for t in &v.trailers {
        if t.reversed {
            break;
        }
        let (back, front) = t.couplings();
        let joint_y = offset.y + back.y;
        offset += back - front;
        out.push((&t.ty.def, offset, joint_y));
    }
    out
}

impl Cabin {
    /// The cabin of a train of vehicles (see [`train_parts`]): the front one's, with the
    /// sections behind joined on as far as they have a cabin and a path network.
    pub(super) fn load_train(parts: &[TrainPart<'_>]) -> Option<Cabin> {
        let (lead, _, _) = parts.first()?;
        let load_cabin = |def: &::legacy_vehicle::Vehicle| -> Option<PassengerCabin> {
            let rel = def.passenger_cabin.as_ref()?;
            PassengerCabin::load(&::legacy_config::resolve_path(def.dir(), rel))
                .map_err(|e| log::warn!("{e}"))
                .ok()
        };
        let load_paths = |def: &::legacy_vehicle::Vehicle| {
            def.paths.as_ref().and_then(|rel| {
                ::legacy_vehicle::VehiclePaths::load(&::legacy_config::resolve_path(def.dir(), rel))
                    .map_err(|e| log::warn!("{e}"))
                    .ok()
            })
        };
        let data = load_cabin(lead)?;
        let mut points: Vec<Vec3> = Vec::new();
        let mut links: Vec<(i32, i32, bool)> = Vec::new();
        let mut link_pack: Vec<Option<usize>> = Vec::new();
        let mut link_room: Vec<f32> = Vec::new();
        let mut step_packs: Vec<Arc<[String]>> = Vec::new();
        // (merged path point or -1, sells tickets, {withbutton}, half width of the section)
        let mut entry_points: Vec<(i32, bool, bool, f32)> = Vec::new();
        let mut exit_points: Vec<(i32, f32)> = Vec::new();
        let mut places: Vec<(::legacy_vehicle::cabin::PassPos, Vec3, usize)> = Vec::new();
        // (the script seat numbers of the sections in front)
        let mut seat_base = 0usize;
        let mut cabin_parts: Vec<CabinPart> = Vec::new();
        // the point of the section in front that leads on to the next one
        let mut rear_link: Option<usize> = None;
        for (k, (def, offset, joint_y)) in parts.iter().enumerate() {
            let cab = if k == 0 {
                Some(data.clone())
            } else {
                load_cabin(def)
            };
            let Some(cab) = cab else { break };
            let (own, own_links, own_steps, own_packs, own_rooms): (
                Vec<Vec3>,
                Vec<(i32, i32, bool)>,
                Vec<i32>,
                Vec<Vec<String>>,
                Vec<f32>,
            ) = match load_paths(def) {
                Some(p) => (
                    p.points
                        .iter()
                        .map(|q| Vec3::from(q.pos) + *offset)
                        .collect(),
                    p.links,
                    p.link_step_sound,
                    p.step_sound_packs,
                    p.link_room_height,
                ),
                None => (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()),
            };
            let base = points.len();
            let valid = |i: i32| (i >= 0 && (i as usize) < own.len()).then_some(base + i as usize);
            let end = |front: bool| {
                (0..own.len())
                    .filter(|i| own[*i].x.abs() < 0.6)
                    .max_by(|a, b| {
                        if front {
                            own[*a].y.total_cmp(&own[*b].y)
                        } else {
                            own[*b].y.total_cmp(&own[*a].y)
                        }
                    })
                    .map(|i| base + i)
            };
            if k > 0 {
                // through the joint: from the front section's [linkToPrevVeh] point to this
                // one's [linkToNextVeh] point (the frontmost aisle point when it has none)
                let front = cab.link_to_next_veh.and_then(valid).or_else(|| end(true));
                match (rear_link, front) {
                    (Some(a), Some(b)) => {
                        links.push((a as i32, b as i32, false));
                        link_pack.push(None);
                        link_room.push(2.0);
                    }
                    // no way through: the section stays empty
                    _ => break,
                }
            }
            points.extend(own.iter().copied());
            links.extend(
                own_links
                    .iter()
                    .map(|(a, b, o)| (a + base as i32, b + base as i32, *o)),
            );
            let pack_base = step_packs.len();
            link_pack.extend((0..own_links.len()).map(|i| {
                let n = own_steps.get(i).copied().unwrap_or(-1);
                (n >= 0 && (n as usize) < own_packs.len()).then(|| pack_base + n as usize)
            }));
            step_packs.extend(own_packs.into_iter().map(Arc::from));
            link_room
                .extend((0..own_links.len()).map(|i| own_rooms.get(i).copied().unwrap_or(2.0)));
            rear_link = cab.link_to_prev_veh.and_then(valid).or_else(|| end(false));
            let half = def
                .bounding_box
                .map(|b| b[0] * 0.5)
                .unwrap_or_else(|| own.iter().map(|p| p.x.abs()).fold(1.2, f32::max));
            let shift = |i: i32| valid(i).map(|m| m as i32).unwrap_or(-1);
            entry_points.extend(
                cab.entries
                    .iter()
                    .map(|e| (shift(e.path_point), !e.no_ticket_sale, e.with_button, half)),
            );
            exit_points.extend(cab.exits.iter().map(|e| (shift(*e), half)));
            places.extend(
                cab.pass_positions
                    .iter()
                    .map(|p| (p.clone(), *offset, seat_base + p.file_index)),
            );
            seat_base += cab.pass_positions.len() + cab.driver_positions.len();
            cabin_parts.push(CabinPart {
                offset: *offset,
                joint_y: *joint_y,
            });
        }
        let graph = PathGraph::new(points.clone(), &links);
        // (the side of the road the stops are on: where a door's own point does not tell)
        let kerb = if LEFT_HAND.load(std::sync::atomic::Ordering::Relaxed) {
            -1.0f32
        } else {
            1.0
        };
        let door = |pp: i32, sells: bool, button: bool, half_width: f32| -> Door {
            let point = (pp >= 0 && (pp as usize) < points.len()).then_some(pp as usize);
            let inside =
                point
                    .map(|i| points[i])
                    .unwrap_or(Vec3::new(kerb * (half_width - 0.1), 4.0, 0.4));
            // A door's side is the side of its entry point; one in the middle of the aisle
            // (or none) is taken to open to the kerb - on the left where the traffic keeps
            // left. (Always the right: a UK bus whose entry point lies on the aisle had the
            // people come to its door from the road side, round the bus.)
            let side = if inside.x.abs() < 0.6 {
                kerb
            } else if inside.x >= 0.0 {
                1.0
            } else {
                -1.0
            };
            let outside = Vec3::new(side * (half_width + DOOR_OUT), inside.y, 0.0);
            // the aisle point next to the door: its neighbour nearest the middle
            let wait_point = point
                .and_then(|i| {
                    graph
                        .neighbours(i)
                        .into_iter()
                        .min_by(|a, b| points[*a].x.abs().total_cmp(&points[*b].x.abs()))
                })
                .filter(|&w| (points[w].x - inside.x).abs() > 0.3);
            // (no aisle point linked beside the door - the W906's door steps lead straight on
            // along it: the nearest path point off the door's line, else a step inwards)
            let wait = wait_point.map(|w| points[w]).unwrap_or_else(|| {
                points
                    .iter()
                    .filter(|p| {
                        (p.x - inside.x).abs() > 0.3
                            && (p.truncate() - inside.truncate()).length() < 1.2
                            && (p.z - inside.z).abs() < 0.6
                    })
                    .min_by(|a, b| {
                        (a.truncate() - inside.truncate())
                            .length()
                            .total_cmp(&(b.truncate() - inside.truncate()).length())
                    })
                    .copied()
                    .unwrap_or(Vec3::new(inside.x - side * 0.7, inside.y, inside.z))
            });
            Door {
                inside,
                point,
                outside,
                side,
                queue_dir: -1.0,
                sells,
                button,
                wait,
            }
        };
        let mut entries: Vec<Door> = entry_points
            .iter()
            .map(|(pp, sells, button, half)| door(*pp, *sells, *button, *half))
            .collect();
        let exits: Vec<Door> = exit_points
            .iter()
            .map(|(pp, half)| door(*pp, false, false, *half))
            .collect();
        // two leaves of one door: the queue of the front leaf runs forwards, the other's back,
        // so that the two lines do not stand in each other
        for i in 0..entries.len() {
            let partner = (0..entries.len()).find(|&j| {
                j != i
                    && entries[j].side == entries[i].side
                    && (entries[j].inside.y - entries[i].inside.y).abs() < 1.4
            });
            entries[i].queue_dir = match partner {
                Some(j) if entries[j].inside.y < entries[i].inside.y => 1.0,
                _ => -1.0,
            };
        }
        let desk = data.ticket_sales.first().map(|ts| {
            let top = Vec3::from(ts.pos);
            let by_point = usize::try_from(ts.path_point)
                .ok()
                .and_then(|i| points.get(i).map(|p| (i, *p)))
                .filter(|(_, p)| (top.truncate() - p.truncate()).length() < 3.0);
            let (stand, pi) = match by_point {
                Some((i, p)) => (p, Some(i)),
                None => {
                    // no usable path point: the nearest one on the entry floor, else the floor by the desk
                    let floor = entries.first().map(|e| e.inside.z).unwrap_or(0.4);
                    let near = points
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| (p.z - floor).abs() < 0.6)
                        .min_by(|a, b| {
                            (a.1.truncate() - top.truncate())
                                .length()
                                .total_cmp(&(b.1.truncate() - top.truncate()).length())
                        });
                    match near {
                        Some((i, p)) => (*p, Some(i)),
                        None => (Vec3::new(top.x + 0.4, top.y, floor), None),
                    }
                }
            };
            let target = match data.driver_positions.first() {
                Some(d) => (top + Vec3::from(d.pos)) * 0.5,
                None => top,
            };
            let d = target - stand;
            let face = if d.truncate().length() > 0.05 {
                (d.x as f64).atan2(d.y as f64).to_degrees()
            } else {
                -90.0
            };
            (stand, pi, face)
        });
        let seats = places
            .iter()
            .map(|(p, offset, omsi_seat)| {
                let pos = Vec3::from(p.pos) + *offset;
                let seated = p.height > 0.01;
                let floor = if seated {
                    let r = p.rot.to_radians();
                    Vec3::new(
                        pos.x + r.sin() * SEAT_FRONT,
                        pos.y + r.cos() * SEAT_FRONT,
                        pos.z - p.height,
                    )
                } else {
                    pos
                };
                Seat {
                    point: points
                        .iter()
                        .enumerate()
                        .filter(|(_, q)| (q.z - floor.z).abs() < 0.75)
                        .min_by(|a, b| {
                            let cost = |q: &Vec3| {
                                let d = *q - floor;
                                d.x * d.x + d.y * d.y + d.z * d.z * 25.0
                            };
                            cost(a.1).total_cmp(&cost(b.1))
                        })
                        .map(|(i, _)| i),
                    pos,
                    floor,
                    rot: p.rot,
                    seated,
                    height: p.height,
                    omsi_seat: *omsi_seat,
                }
            })
            .collect();
        let routes = build_routes(&graph, &links);
        let invalid_entries = entries.iter().filter(|e| e.point.is_none()).count();
        let invalid_exits = exits.iter().filter(|e| e.point.is_none()).count();
        if invalid_entries > 0 || invalid_exits > 0 {
            log::warn!(
                "{}: passenger cabin has {invalid_entries} entries and {invalid_exits} exits without valid path points",
                lead.path.display()
            );
        }
        let point_of = |i: i32| usize::try_from(i).ok().filter(|i| *i < graph.points.len());
        let stamper = data
            .stampers
            .last()
            .map(|st| (point_of(st.path_point), Vec3::from(st.pos)));
        let money_point = data.money_points.last().map(|m| Vec3::from(m.pos));
        // (the Citaro's lies on the floor: passengers bent down to their feet for it)
        let sale = data.ticket_sales.last().map(|st| {
            let pt = point_of(st.path_point);
            let floor = pt.and_then(|k| points.get(k)).map(|q| q.z);
            let pos = Vec3::from(st.pos);
            let low = floor.is_some_and(|z| pos.z - z < 0.6);
            (pt, money_point.filter(|_| low).unwrap_or(pos))
        });
        let money_var = data.money_points.last().map(|m| (Vec3::from(m.pos), m.var));
        let change_point = data.change_points.last().map(|m| Vec3::from(m.pos));
        Some(Cabin {
            data,
            graph,
            links,
            link_pack,
            step_packs,
            entries,
            exits,
            desk,
            seats,
            parts: cabin_parts,
            link_room,
            routes,
            stamper,
            sale,
            money_point,
            money_var,
            change_point,
        })
    }

    /// Every point of the path network (Omsi.exe's list +0xc of the paths).
    pub(super) fn all_points(&self) -> Vec<Option<usize>> {
        (0..self.graph.points.len()).map(Some).collect()
    }
}

/// Whether the straight way from `a` to `b` goes over a carriageway: across the centre
/// line of a street lane (walking along the kerb on the carriageway's edge does not).
pub(super) fn crosses_street(net: &Network, a: DVec2, b: DVec2) -> bool {
    let mut cells: Vec<(i32, i32)> = Vec::new();
    for p in [a, b, (a + b) * 0.5] {
        let c = Network::grid_cell(p.extend(0.0));
        if !cells.contains(&c) {
            cells.push(c);
        }
    }
    let mut seen: Vec<usize> = Vec::new();
    for c in cells {
        for &i in net.grid.get(&c).map(|v| v.as_slice()).unwrap_or(&[]) {
            if seen.contains(&i) {
                continue;
            }
            seen.push(i);
            let l = &net.lanes[i];
            if l.kind != LaneKind::Street {
                continue;
            }
            if l.points
                .windows(2)
                .any(|w| segments_cross(a, b, w[0].truncate(), w[1].truncate()))
            {
                return true;
            }
        }
    }
    false
}

/// Whether the segments `a`-`b` and `c`-`d` cross.
pub(super) fn segments_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let side = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
    let (d1, d2) = (side(c, d, a), side(c, d, b));
    let (d3, d4) = (side(a, b, c), side(a, b, d));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// A bus as the passengers see it this frame.
#[derive(Clone)]
pub(super) struct BusNow {
    pub(super) id: BusId,
    pub(super) cabin: Arc<Cabin>,
    pub(super) pos: DVec3,
    pub(super) rot: Mat4,
    pub(super) heading: f64,
    /// m/s, forwards.
    pub(super) speed: f64,
    pub(super) entry_open: Vec<bool>,
    pub(super) exit_open: Vec<bool>,
    /// The doors a walker may use (another player's bus: its doors as they are, while
    /// `entry_open` stays shut for the passengers here); None: as `entry_open`/`exit_open`.
    pub(super) walk_open: Option<(Vec<bool>, Vec<bool>)>,
    pub(super) interior: f32,
    /// The saloon's air and the light outside, for what boarding passengers say.
    pub(super) air: CabinAir,
    /// Half extents across / along and the centre of its bounding box (bus frame).
    pub(super) half: DVec2,
    pub(super) centre: DVec2,
    /// Acceleration of the floor (bus frame: x to the right, y forwards; m/s²).
    pub(super) accel: DVec2,
    /// The sections behind the front one (the cabin's parts after the first).
    pub(super) trailers: Vec<PartFrame>,
    /// The terminus it shows, by its HOF identity. None means its target is unknown.
    pub(super) terminus: Option<String>,
    /// Explicit `$allexit$` service; a missing/unknown target is not out of service.
    pub(super) out_of_service: bool,
}

/// What passengers feel stepping into a bus (OMSI reads the same fields: the vehicle's
/// `Cabinair_Temp` and `Cabinair_relHum`, the weather's temperature and the daylight).
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct CabinAir {
    /// °C, when the bus keeps its cabin air (every bus does: its script or the engine).
    pub(super) temp: Option<f32>,
    /// Relative humidity, a fraction.
    pub(super) rel_hum: f32,
    /// The temperature outside (°C).
    pub(super) outside: f32,
    /// `Envir_Brightness`: the daylight, 0 dark .. 1.
    pub(super) brightness: f32,
}

impl CabinAir {
    pub(super) fn of(v: &VehicleInstance) -> CabinAir {
        CabinAir {
            temp: v.var("Cabinair_Temp").filter(|t| t.is_finite()),
            rel_hum: v
                .var("Cabinair_relHum")
                .filter(|h| h.is_finite())
                .unwrap_or(0.0),
            outside: v.host.temperature,
            brightness: v.var("Envir_Brightness").unwrap_or(1.0),
        }
    }
}

/// Where a rear section of a bus is this frame, with its place in the cabin.
#[derive(Debug, Clone, Copy)]
pub(super) struct PartFrame {
    pub(super) pos: DVec3,
    pub(super) rot: Mat4,
    pub(super) heading: f64,
    pub(super) offset: Vec3,
    pub(super) joint_y: f32,
    /// Half extents across / along and the centre of its bounding box (own frame).
    pub(super) half: DVec2,
    pub(super) centre: DVec2,
}

/// The rear sections of `v` that are parts of `cabin`, as they stand now.
pub(super) fn part_frames(v: &VehicleInstance, cabin: &Cabin) -> Vec<PartFrame> {
    cabin
        .parts
        .iter()
        .skip(1)
        .zip(&v.trailers)
        .map(|(cp, t)| {
            let bb =
                t.ty.def
                    .bounding_box
                    .unwrap_or([2.5, 7.0, 3.0, 0.0, 0.0, 1.5]);
            PartFrame {
                pos: t.position,
                rot: t.body_rotation(),
                heading: t.heading,
                offset: cp.offset,
                joint_y: cp.joint_y,
                half: DVec2::new(bb[0] as f64 * 0.5, bb[1] as f64 * 0.5),
                centre: DVec2::new(bb[3] as f64, bb[4] as f64),
            }
        })
        .collect()
}

/// How far into the frame of the section behind joint `t` a cabin point `y` lies: 0 in
/// front of the joint's blend, 1 behind it.
pub(super) fn behind(t: &PartFrame, y: f32) -> f32 {
    ((JOINT_BLEND - (y - t.joint_y)) / (2.0 * JOINT_BLEND)).clamp(0.0, 1.0)
}

/// Where a point of a cabin (unfolded frame) is in the world: the front section carries
/// what lies ahead of the first joint, a rear section what lies behind its joint, and near
/// a joint the two are blended, so that somebody walking through the bellows moves on
/// smoothly however far the bus is bent.
pub(super) fn train_point(pos: DVec3, rot: &Mat4, trailers: &[PartFrame], local: Vec3) -> DVec3 {
    let mut here = pos + rot.transform_point3(local).as_dvec3();
    for t in trailers {
        let w = behind(t, local.y);
        if w <= 0.0 {
            break;
        }
        let there = t.pos + t.rot.transform_point3(local - t.offset).as_dvec3();
        here = here.lerp(there, w as f64);
        if w < 1.0 {
            break;
        }
    }
    here
}

/// The heading of the floor at a point of a cabin (see [`train_point`]).
pub(super) fn train_heading(heading: f64, trailers: &[PartFrame], local: Vec3) -> f64 {
    let mut here = heading;
    for t in trailers {
        let w = behind(t, local.y);
        if w <= 0.0 {
            break;
        }
        here += crowd::angle_diff(here, t.heading) * w as f64;
        if w < 1.0 {
            break;
        }
    }
    here
}

impl BusNow {
    pub(super) fn world(&self, local: Vec3) -> DVec3 {
        train_point(self.pos, &self.rot, &self.trailers, local)
    }
    /// A world point in the cabin's frame (the inverse of `world`): the front section's,
    /// or a rear section's for a point behind its joint.
    pub(super) fn to_local(&self, w: DVec3) -> Vec3 {
        let mut l = self
            .rot
            .inverse()
            .transform_point3((w - self.pos).as_vec3());
        for t in &self.trailers {
            if l.y > t.joint_y {
                break;
            }
            l = t.rot.inverse().transform_point3((w - t.pos).as_vec3()) + t.offset;
        }
        l
    }
    /// The tilt (pitch and bank, in the world's axes, no heading) of the section a point of
    /// the cabin is in.
    pub(super) fn tilt_at(&self, local: Vec3) -> Mat4 {
        let mut rot = self.rot;
        let mut heading = self.heading;
        for t in &self.trailers {
            if behind(t, local.y) < 0.5 {
                break;
            }
            rot = t.rot;
            heading = t.heading;
        }
        rot * Mat4::from_rotation_z(heading.to_radians() as f32)
    }
    /// The heading of the section a point of the cabin is in.
    pub(super) fn heading_at(&self, local: Vec3) -> f64 {
        train_heading(self.heading, &self.trailers, local)
    }
    pub(super) fn boarding_open(&self, door: usize) -> bool {
        match door.checked_sub(self.cabin.entries.len()) {
            None => self.entry_open.get(door),
            Some(exit) => self.exit_open.get(exit),
        }
        .copied()
        .unwrap_or(false)
    }
    pub(super) fn fwd(&self) -> DVec2 {
        let h = self.heading.to_radians();
        DVec2::new(h.sin(), h.cos())
    }
    /// The bodies people on the ground walk round: the bus and its rear sections.
    pub(super) fn blocks(&self) -> Vec<Block> {
        let block = |pos: DVec3, heading: f64, half: DVec2, centre: DVec2| {
            let h = heading.to_radians();
            let (fwd, right) = (DVec2::new(h.sin(), h.cos()), DVec2::new(h.cos(), -h.sin()));
            Block {
                center: pos.truncate() + right * centre.x + fwd * centre.y,
                half,
                heading: h,
                vel: fwd * self.speed,
            }
        };
        let mut out = vec![block(self.pos, self.heading, self.half, self.centre)];
        out.extend(
            self.trailers
                .iter()
                .map(|t| block(t.pos, t.heading, t.half, t.centre)),
        );
        out
    }
}

pub(in crate::humans) struct PassengerBuses {
    /// Passenger cabins by vehicle files (the front vehicle and its coupled parts).
    pub(in crate::humans) cabins: HashMap<Vec<PathBuf>, Option<Arc<Cabin>>>,
    pub(in crate::humans) player_cabin: Option<Arc<Cabin>>,
    /// Which places of each bus are taken.
    pub(in crate::humans) seats: HashMap<BusId, Vec<bool>>,
    /// Kilometres each bus has driven (the odometer the riders read, +0x430).
    pub(in crate::humans) odometer: HashMap<BusId, f64>,
    /// The `PAX_Entry<n>_Req` / `PAX_Exit<n>_Req` of each bus this frame.
    pub(in crate::humans) pax_req: HashMap<BusId, (Vec<bool>, Vec<bool>)>,
    /// Stop the player's bus is serving (standing at it).
    pub(in crate::humans) served_stop: Option<i64>,
    /// Timetable buses at a stop: id → (stop, time the visit began).
    pub(in crate::humans) ai_visits: HashMap<u64, (i64, f64)>,
    /// When each bus last had a door open (the passengers' clock).
    pub(in crate::humans) last_door_open: HashMap<BusId, f64>,
    /// Timetable buses to keep at their stop for a few seconds more (for the traffic), and
    /// whether somebody is crossing one of its doorways.
    pub(in crate::humans) holds: Vec<(u64, f32, bool)>,
    /// Door requests for the timetable buses' scripts: (bus, entries, exits).
    pub(in crate::humans) ai_requests: Vec<(u64, Vec<bool>, Vec<bool>)>,
    /// The buses of the last tick (for the avatars' seats and doors).
    pub(in crate::humans) last_buses: Vec<BusNow>,
    /// The vehicles the player placed and is not driving now (`placed_bus_id`): their
    /// riders stay in them when the player drives another.
    pub(in crate::humans) placed_now: Vec<BusNow>,
    /// Speed, heading and floor acceleration of each bus last frame (for the riders' balance).
    pub(in crate::humans) bus_motion: HashMap<BusId, (f64, f64, DVec2)>,
}

impl Humans {
    /// The cabin of a vehicle with the parts coupled behind it.
    pub(super) fn cabin_for(&mut self, v: &VehicleInstance) -> Option<Arc<Cabin>> {
        let parts = train_parts(v);
        let key: Vec<PathBuf> = parts.iter().map(|p| p.0.path.clone()).collect();
        if let Some(c) = self.buses.cabins.get(&key) {
            return c.clone();
        }
        let cabin = Cabin::load_train(&parts).map(|mut c| {
            let entries = c.entries.len();
            for (k, exit) in c.exits.iter_mut().enumerate() {
                exit.button =
                    v.ty.program
                        .var(&format!("PAX_Entry{}_Req", entries + k))
                        .is_some_and(|id| v.ty.program.reads(id));
            }
            Arc::new(c)
        });
        if let Some(c) = cabin.as_ref().filter(|c| c.parts.len() > 1) {
            log::info!(
                "passenger cabin of {}: {} sections joined ({} places, {} entries, {} exits, {} path points)",
                v.ty.def
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                c.parts.len(),
                c.seats.len(),
                c.entries.len(),
                c.exits.len(),
                c.graph.points.len()
            );
        }
        self.buses.cabins.insert(key, cabin.clone());
        cabin
    }

    /// Whether the bus script reports `name`: it writes it (`PAX_*` are engine variables every
    /// vehicle has, so stock scripts set them without a varlist entry) or declares it.
    pub(super) fn script_reports(v: &VehicleInstance, name: &str) -> bool {
        v.has_script_var(name)
            || v.ty
                .program
                .var(name)
                .is_some_and(|id| v.ty.program.stores(id))
    }

    /// `PAX_Entry<i>_Open` / `PAX_Exit<i>_Open` as the bus script reports them. A bus whose
    /// script never sets them (or only sets some of them) falls back to its physical `door_<i>`
    /// or `door<i>` animations.
    pub(super) fn doors_open(
        v: &VehicleInstance,
        n_entry: usize,
        n_exit: usize,
    ) -> (Vec<bool>, Vec<bool>) {
        let door_val = |k: usize| -> bool {
            v.var(&format!("door_{k}"))
                .or_else(|| v.var(&format!("door{k}")))
                .unwrap_or(0.0)
                > 0.5
        };
        // Exits in standard OMSI city buses (2 or more front door leaves) begin at door_2 (middle door),
        // while coaches with a single front door leaf begin at door_1. Exits must not be offset by
        // n_entry, because buses with all doors configured as entries (e.g. 3-door buses with 6 entries)
        // still place middle-door exits at door_2/3 and rear-door exits at door_4/5.
        let exit_door_base = if n_entry <= 1 { 1 } else { 2 };
        let entry: Vec<bool> = (0..n_entry)
            .map(|i| {
                let name = format!("PAX_Entry{i}_Open");
                if Self::script_reports(v, &name) {
                    v.var(&name).unwrap_or(0.0) > 0.5
                } else {
                    door_val(i)
                }
            })
            .collect();
        let exit: Vec<bool> = (0..n_exit)
            .map(|i| {
                let name = format!("PAX_Exit{i}_Open");
                if Self::script_reports(v, &name) {
                    v.var(&name).unwrap_or(0.0) > 0.5
                } else {
                    door_val(exit_door_base + i)
                }
            })
            .collect();
        (entry, exit)
    }

    pub(crate) fn any_door_open(v: &VehicleInstance) -> bool {
        let (entry, exit) = Self::doors_open(v, 8, 8);
        entry.into_iter().chain(exit).any(|open| open)
    }

    /// The buses passengers deal with this frame.
    pub(super) fn gather_buses(
        &mut self,
        world: &World,
        bus: Option<&VehicleInstance>,
        traffic: Option<&Traffic>,
    ) -> Vec<BusNow> {
        let mut out = Vec::new();
        let stops: Vec<(i64, DVec3, f64)> = world
            .bus_stops
            .lock()
            .iter()
            .map(|s| (s.0, s.1, s.2))
            .collect();
        // The stop a bus serves: the nearest in reach - but one facing the way the bus goes
        // before one facing the other way. The two stops of a street often lie within
        // reach of each other, and the people of the stop across the road then walked over
        // the carriageway, through the traffic, to a bus that was not theirs.
        let serving = |pos: DVec3, heading: f64, reach: f64| -> Option<i64> {
            stops
                .iter()
                .filter(|s| (s.1 - pos).length() < reach)
                .min_by(|a, c| {
                    let back =
                        |s: &(i64, DVec3, f64)| crowd::angle_diff(heading, s.2).abs() > 100.0;
                    back(a)
                        .cmp(&back(c))
                        .then((a.1 - pos).length().total_cmp(&(c.1 - pos).length()))
                })
                .map(|s| s.0)
        };
        let bb_of = |v: &VehicleInstance| {
            let bb =
                v.ty.def
                    .bounding_box
                    .unwrap_or([2.5, 11.0, 3.0, 0.0, 0.0, 1.5]);
            (
                DVec2::new(bb[0] as f64 * 0.5, bb[1] as f64 * 0.5),
                DVec2::new(bb[3] as f64, bb[4] as f64),
            )
        };
        if let (Some(b), Some(cabin)) = (bus, self.buses.player_cabin.clone()) {
            let speed = b.physics.velocity_kmh() as f64 / 3.6;
            let (entry_open, exit_open) =
                Self::doors_open(b, cabin.entries.len(), cabin.exits.len());
            let (half, centre) = bb_of(b);
            let trailers = part_frames(b, &cabin);
            let target = if Self::script_reports(b, "target_index_int") {
                b.var("target_index_int")
            } else {
                b.var("IBIS_TerminusIndex")
            };
            let service = match (target, b.host.hof.as_ref()) {
                (Some(i), Some(hof)) if i.is_finite() && i >= 0.0 => hof
                    .termini
                    .get(i.round() as usize)
                    .map(|t| (t.texture_id.trim().to_string(), t.all_exit)),
                _ => None,
            };
            let out_of_service = service.as_ref().is_some_and(|s| s.1);
            let terminus = service.filter(|s| !s.1).map(|s| s.0);
            out.push(BusNow {
                terminus,
                out_of_service,
                id: BusId::Player,
                walk_open: None,
                cabin,
                pos: b.position,
                rot: b.body_rotation(),
                heading: b.heading,
                speed,
                entry_open,
                exit_open,
                interior: b.interior_light(),
                air: CabinAir::of(b),
                half,
                centre,
                accel: DVec2::ZERO,
                trailers,
            });
        }
        if let Some(t) = traffic {
            let near = self.center;
            let riding: HashSet<u64> = self
                .people
                .iter()
                .filter_map(|p| match p.state.bus() {
                    Some(BusId::Ai(id)) => Some(id),
                    _ => None,
                })
                .collect();
            let mut visits = HashMap::new();
            for c in t.cars().iter().filter(|c| c.is_bus()) {
                let from_eye = self
                    .eye
                    .map(|e| (c.vehicle.position - e.pos).length())
                    .unwrap_or(f64::MAX);
                if (c.vehicle.position - near).length().min(from_eye) > 400.0
                    && !riding.contains(&c.id.get())
                {
                    continue;
                }
                let Some(cabin) = self.cabin_for(&c.vehicle) else {
                    continue;
                };
                let speed = c.state.speed as f64;
                let stop = if c.boarding_permission() && speed.abs() < 0.3 {
                    serving(c.vehicle.position, c.vehicle.heading, 18.0)
                } else {
                    None
                };
                let since = match stop {
                    Some(s) => {
                        let v = match self.buses.ai_visits.get(&c.id.get()) {
                            Some(&(vs, t0)) if vs == s => (vs, t0),
                            _ => (s, self.time),
                        };
                        visits.insert(c.id.get(), v);
                        self.time - v.1
                    }
                    None => 0.0,
                };
                let open = stop.is_some();
                let (mut entry_open, mut exit_open) = (
                    vec![false; cabin.entries.len()],
                    vec![false; cabin.exits.len()],
                );
                if open {
                    if Self::script_reports(&c.vehicle, "PAX_Entry0_Open")
                        || c.vehicle.var("door_0").is_some()
                        || c.vehicle.var("door0").is_some()
                    {
                        let (e, x) =
                            Self::doors_open(&c.vehicle, cabin.entries.len(), cabin.exits.len());
                        entry_open = e;
                        exit_open = x;
                    } else if since > 2.5 {
                        // the script does not say: the doors are open while the bus boards
                        entry_open
                            .iter_mut()
                            .chain(exit_open.iter_mut())
                            .for_each(|o| *o = true);
                    }
                }
                self.buses
                    .seats
                    .entry(BusId::Ai(c.id.get()))
                    .or_insert_with(|| vec![false; cabin.seats.len()]);
                let (half, centre) = bb_of(&c.vehicle);
                let trailers = part_frames(&c.vehicle, &cabin);
                out.push(BusNow {
                    terminus: c
                        .bus
                        .as_ref()
                        .map(|b| b.terminus.trim().to_string())
                        .filter(|t| !t.is_empty()),
                    out_of_service: false,
                    id: BusId::Ai(c.id.get()),
                    walk_open: None,
                    cabin,
                    pos: c.vehicle.position,
                    rot: c.vehicle.body_rotation(),
                    heading: c.vehicle.heading,
                    speed,
                    entry_open,
                    exit_open,
                    interior: c.vehicle.interior_light(),
                    air: CabinAir::of(&c.vehicle),
                    half,
                    centre,
                    accel: DVec2::ZERO,
                    trailers,
                });
            }
            self.buses.ai_visits = visits;
            let alive: HashSet<u64> = t
                .cars()
                .iter()
                .map(|c| c.id.get())
                .chain(
                    self.network
                        .remote_now
                        .iter()
                        .chain(self.buses.placed_now.iter())
                        .filter_map(|b| match b.id {
                            BusId::Ai(id) => Some(id),
                            BusId::Player => None,
                        }),
                )
                .collect();
            self.buses.seats.retain(|k, _| match k {
                BusId::Ai(id) => alive.contains(id),
                BusId::Player => true,
            });
        }
        for b in self
            .network
            .remote_now
            .iter()
            .chain(self.buses.placed_now.iter())
        {
            self.buses
                .seats
                .entry(b.id)
                .or_insert_with(|| vec![false; b.cabin.seats.len()]);
            out.push(b.clone());
        }
        out
    }

    /// A bus people may be in but do not board here (another player's, one the player left).
    pub(super) fn parked_bus(&mut self, id: BusId, v: &VehicleInstance) -> Option<BusNow> {
        let cabin = self.cabin_for(v)?;
        let bb =
            v.ty.def
                .bounding_box
                .unwrap_or([2.5, 11.0, 3.0, 0.0, 0.0, 1.5]);
        let trailers = part_frames(v, &cabin);
        let walk_open = Self::doors_open(v, cabin.entries.len(), cabin.exits.len());
        Some(BusNow {
            terminus: None,
            out_of_service: false,
            id,
            entry_open: vec![false; cabin.entries.len()],
            exit_open: vec![false; cabin.exits.len()],
            walk_open: Some(walk_open),
            cabin,
            pos: v.position,
            rot: v.body_rotation(),
            heading: v.heading,
            speed: v.physics.velocity_kmh() as f64 / 3.6,
            interior: v.interior_light(),
            air: CabinAir::of(v),
            half: DVec2::new(bb[0] as f64 * 0.5, bb[1] as f64 * 0.5),
            centre: DVec2::new(bb[3] as f64, bb[4] as f64),
            accel: DVec2::ZERO,
            trailers,
        })
    }

    /// Seats of the player's bus from its `[passengercabin]`, and the engine's side of the
    /// ticket printer: `GivenTicket` is -1 until the driver hands a ticket over (the stock
    /// `Ticketprinter.osc` never sets it, OMSI starts it at -1 - left at 0 the first
    /// passenger took ticket 0 without the driver doing anything).
    pub fn set_cabin(&mut self, vehicle: &mut VehicleInstance) {
        vehicle.set_engine_var("GivenTicket", -1.0);
        match self.cabin_for(vehicle) {
            Some(c) => {
                let exit_base = if c.entries.len() <= 1 { 1 } else { 2 };
                for (role, count, base) in [
                    ("Entry", c.entries.len(), 0),
                    ("Exit", c.exits.len(), exit_base),
                ] {
                    for i in 0..count {
                        if !Self::script_reports(vehicle, &format!("PAX_{role}{i}_Open"))
                            && vehicle.var(&format!("door_{}", base + i)).is_none()
                            && vehicle.var(&format!("door{}", base + i)).is_none()
                        {
                            log::warn!(
                                "{}: {role} {i} has no passenger door report or supported physical door variable",
                                vehicle.ty.def.path.display()
                            );
                        }
                    }
                }
                log::info!(
                    "passenger cabin: {} places ({} seats), {} entries, {} exits, {} path points, desk {:?}",
                    c.seats.len(),
                    c.seats.iter().filter(|s| s.seated).count(),
                    c.entries.len(),
                    c.exits.len(),
                    c.graph.points.len(),
                    c.desk.map(|d| d.0)
                );
                if debug_pax() {
                    for (i, e) in c.entries.iter().enumerate() {
                        log::info!(
                            "  entry {i}: inside {:?} wait {:?} sells {}",
                            e.inside,
                            e.wait,
                            e.sells
                        );
                    }
                    for (i, e) in c.exits.iter().enumerate() {
                        log::info!("  exit {i}: inside {:?} wait {:?}", e.inside, e.wait);
                    }
                    for (i, s) in c.seats.iter().enumerate() {
                        log::info!(
                            "  seat {i}: pos {:?} floor {:?} rot {:.0} seated {}",
                            s.pos,
                            s.floor,
                            s.rot,
                            s.seated
                        );
                    }
                }
                self.buses
                    .seats
                    .insert(BusId::Player, vec![false; c.seats.len()]);
                self.entry_req = vec![false; c.entries.len().max(1)];
                self.exit_req = vec![false; c.exits.len().max(1)];
                self.buses.player_cabin = Some(c);
            }
            None => log::info!("{}: no passenger cabin", vehicle.ty.def.path.display()),
        }
    }

    /// The vehicles standing in the world that the player placed and does not drive now:
    /// (their `Player::uid`, the vehicle). Their riders stay aboard; nobody new boards them.
    pub fn set_placed_buses<'a>(
        &mut self,
        buses: impl Iterator<Item = (u64, &'a VehicleInstance)>,
    ) {
        let mut out = Vec::new();
        for (uid, v) in buses {
            if let Some(b) = self.parked_bus(BusId::Ai(placed_bus_id(uid)), v) {
                out.push(b);
            }
        }
        self.buses.placed_now = out;
    }

    /// The player now drives vehicle `new_uid` and left `old_uid`: whoever rode in the one
    /// left stays in it (it is one of the placed vehicles now), whoever rode in the one
    /// taken over is the player's bus's, and the player's cabin is the new vehicle's own.
    /// (Riders followed the player into the next bus, and people boarding it took the old
    /// bus's seats - places in the air round a minibus's bonnet.)
    pub fn player_bus_swapped(
        &mut self,
        old_uid: u64,
        new_uid: u64,
        new_vehicle: &mut VehicleInstance,
    ) {
        let old = BusId::Ai(placed_bus_id(old_uid));
        let new = BusId::Ai(placed_bus_id(new_uid));
        // (through a free id: Player -> old, new -> Player)
        let tmp = BusId::Ai(u64::MAX);
        self.remap_bus(BusId::Player, tmp);
        self.remap_bus(new, BusId::Player);
        self.remap_bus(tmp, old);
        let kept = self.buses.seats.remove(&BusId::Player);
        self.buses.player_cabin = None;
        self.buses.served_stop = None;
        self.set_cabin(new_vehicle);
        if let (Some(k), Some(now)) = (kept, self.buses.seats.get_mut(&BusId::Player)) {
            if k.len() == now.len() {
                *now = k;
            }
        }
    }

    /// Bus `bus` is gone (the player removed it): whoever was in it stands where they were,
    /// on the ground, and walks off.
    pub fn evict(&mut self, bus: BusId, world: &World) {
        let _ = world;
        for i in (0..self.people.len()).rev() {
            let p = &self.people[i];
            let theirs = matches!(p.place, Place::Bus(b, _) if b == bus)
                || matches!(&p.state, State::Pax(x) if x.bus == Some(bus) || x.inside == Some(bus));
            if theirs {
                self.release(i);
                let p = self.people.swap_remove(i);
                self.retire(&p);
            }
        }
        self.buses.seats.remove(&bus);
        self.buses.bus_motion.remove(&bus);
        if bus == BusId::Player {
            self.buses.player_cabin = None;
            self.buses.served_stop = None;
        }
    }

    /// Everyone and everything that belongs to bus `from` belongs to `to` now.
    pub(super) fn remap_bus(&mut self, from: BusId, to: BusId) {
        let fix = |b: &mut BusId| {
            if *b == from {
                *b = to;
            }
        };
        for p in &mut self.people {
            if let Place::Bus(b, _) = &mut p.place {
                fix(b);
            }
            if let State::Pax(x) = &mut p.state {
                if let Some(b) = x.bus.as_mut() {
                    fix(b);
                }
                if let Some(b) = x.inside.as_mut() {
                    fix(b);
                }
            }
        }
        if let Some(v) = self.buses.seats.remove(&from) {
            self.buses.seats.insert(to, v);
        }
        if let Some(v) = self.buses.bus_motion.remove(&from) {
            self.buses.bus_motion.insert(to, v);
        }
    }
}
