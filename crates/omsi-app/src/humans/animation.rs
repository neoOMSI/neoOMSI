use super::{BusId, BusNow, Humans, Person, Place, PuppetMode, State, Task, model_point, pax};
use crate::{ambience, scene::World};
use glam::{DVec2, DVec3, Vec3};
use hashbrown::HashMap;
use omsi_sim::{crowd, human::Activity, human_omsi::AnimInput};

fn active_footstep(procedural: bool, valid: bool, landed: bool, legacy_step: bool) -> bool {
    if procedural && valid {
        landed
    } else {
        legacy_step
    }
}

fn pose_velocity(person: &Person, frame: u64, origin: DVec3, dt: f32) -> DVec2 {
    let (previous_frame, previous_origin, _) = person.pose.floor_pose();
    if person.remote
        && matches!(person.place, Place::Bus(..))
        && previous_frame == frame
        && dt > 0.0
    {
        (origin - previous_origin).truncate() / dt as f64
    } else {
        person.vel
    }
}

impl Person {
    /// Cache the bones selected for this frame so footstep timing and drawing agree,
    /// including the existing invalid-procedural-pose fallback.
    pub(super) fn finish_animation(&mut self, procedural: bool, input: &AnimInput) -> bool {
        let legacy = self.anim.advance(&self.ty.omsi, input);
        let posed = self.pose.bones(&self.ty.rig);
        self.render.active_bones = Some(if procedural && posed.ok {
            posed.bones
        } else {
            omsi_sim::human::slots_from_omsi(&self.anim.bones(&self.ty.omsi))
        });
        active_footstep(
            procedural,
            posed.ok,
            self.pose.landed() && self.vel.length() > 0.25,
            legacy.step,
        )
    }
}

impl Humans {
    /// Everybody's animation this frame (sub_626ae8): the passengers from what their task
    /// says (`PAX_State`, speed, the room height, the seat, the hand and the head), the
    /// pedestrians from their walk.
    pub(super) fn animate(
        &mut self,
        dt: f32,
        world: &World,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
    ) {
        let dt_ms = dt * 1000.0;
        for i in 0..self.people.len() {
            if let Some(pp) = self.people[i].puppet {
                if pp.mode == PuppetMode::Avatar {
                    self.animate_avatar(i, dt, world, buses, bus_ix);
                }
                continue;
            }
            let p = &self.people[i];
            let bn = match p.place {
                Place::Bus(b, _) => bus_ix.get(&b).map(|k| &buses[*k]),
                Place::Ground => None,
            };
            let (frame, origin, heading) = match (p.place, bn) {
                (Place::Bus(b, l), Some(_)) => (b.space(), l.as_dvec3(), p.lheading),
                _ => (0, p.position, p.heading),
            };
            let to_model = |q: DVec3| model_point(origin, heading, q);
            let velocity = pose_velocity(p, frame, origin, dt);
            let mut ik_activity = p.activity;
            let mut ik_seat: Option<Vec3> = None;
            let mut ik_look: Option<Vec3> = None;
            let mut ik_reach: Option<Vec3> = None;
            let mut ik_hold = 0.0;
            let mut facing: Option<f64> = None;

            let (input, footstep) = match &p.state {
                State::Pax(x) => {
                    let bn = x.inside.and_then(|b| bus_ix.get(&b).map(|k| &buses[*k]));
                    // a point of the bus in the person's own frame, Direct3D's axes
                    let own = |q: Vec3| -> Vec3 {
                        let v = q.as_dvec3() - x.pos;
                        let (s, c) = x.yaw.sin_cos();
                        let local = Vec3::new(
                            (v.x * c - v.y * s) as f32,
                            (v.x * s + v.y * c) as f32,
                            v.z as f32,
                        );
                        omsi_sim::human_omsi::d3d(local)
                    };
                    let reach = (x.reach && x.inside.is_some()).then(|| own(x.reach_at));
                    let look = match (x.look_driver, bn) {
                        (true, Some(b)) => b
                            .cabin
                            .data
                            .driver_positions
                            .first()
                            .map(|d| own(Vec3::from(d.pos) + Vec3::Z * 0.65)),
                        _ => None,
                    };
                    if x.reach && x.inside.is_some() {
                        ik_reach = Some(to_model(x.reach_at.as_dvec3()));
                    }
                    if x.look_driver {
                        if let Some(b) = bn {
                            ik_look =
                                b.cabin.data.driver_positions.first().map(|d| {
                                    to_model(Vec3::from(d.pos).as_dvec3() + DVec3::Z * 0.65)
                                });
                        }
                    }
                    if x.task == Task::SittingInBus {
                        if let Some(s) = x.seat.and_then(|k| bn.and_then(|b| b.cabin.seats.get(k)))
                        {
                            if s.seated {
                                if x.seat_approach.is_some() {
                                    ik_activity = if x.speed > 0.1 {
                                        Activity::Walk
                                    } else {
                                        Activity::Stand
                                    };
                                } else {
                                    ik_activity = Activity::Sit;
                                    ik_seat = Some(to_model(s.pos.as_dvec3()));
                                }
                                facing = Some(s.rot as f64);
                            } else {
                                ik_activity = Activity::Stand;
                                if bn.is_some_and(|b| b.speed.abs() > 0.4 || b.accel.length() > 0.4)
                                {
                                    ik_hold = 1.0;
                                }
                            }
                        }
                    } else if x.task == Task::InBusToExit {
                        ik_activity = if x.speed > 0.1 {
                            Activity::Walk
                        } else {
                            Activity::Stand
                        };
                        if bn.is_some_and(|b| b.speed.abs() > 0.4 || b.accel.length() > 0.4) {
                            ik_hold = 1.0;
                        }
                    } else if x.task == Task::WaitingForBus {
                        ik_activity = if x.seat_approach.is_some() && x.speed > 0.1 {
                            Activity::Walk
                        } else if x.posture == pax::Posture::Sitting {
                            Activity::Sit
                        } else {
                            Activity::Stand
                        };
                        if ik_activity == Activity::Sit {
                            ik_seat = x
                                .stop
                                .zip(x.spot)
                                .and_then(|(s, k)| self.stops.get(&s).and_then(|s| s.spots.get(k)))
                                .map(|sp| to_model(sp.pos));
                        }
                        let near = buses
                            .iter()
                            .filter(|b| b.speed.abs() < 14.0)
                            .map(|b| (b, (b.pos - p.position).length()))
                            .filter(|(_, d)| *d < 35.0)
                            .min_by(|a, b| a.1.total_cmp(&b.1));
                        if let Some((b, _)) = near {
                            let aim = b
                                .cabin
                                .entries
                                .first()
                                .map(|e| b.world(e.inside))
                                .unwrap_or(b.pos);
                            ik_look = Some(to_model(aim + DVec3::Z * 1.3));
                        }
                    } else {
                        let v = x.speed;
                        ik_activity = if v > 0.1 {
                            Activity::Walk
                        } else {
                            Activity::Stand
                        };
                    }
                    let kind = x.posture as u8;
                    let pack = match (x.step_pack, bn) {
                        (Some(k), Some(b)) => {
                            b.cabin.step_packs.get(k).cloned().map(|pk| (b.id, pk))
                        }
                        _ => None,
                    };
                    (
                        AnimInput {
                            kind,
                            speed: x.speed,
                            moved: x.moved,
                            room_height: x.room,
                            seat_height: x.seat_h,
                            reach,
                            look,
                            smooth: x.smooth,
                            dt_ms,
                        },
                        pack,
                    )
                }
                State::Strolling(_) => {
                    let v = p.vel.length() as f32;
                    ik_activity = if v > 0.1 {
                        Activity::Walk
                    } else {
                        Activity::Stand
                    };
                    (
                        AnimInput {
                            kind: if v > 0.05 { 1 } else { 0 },
                            speed: v,
                            moved: v * dt,
                            room_height: pax::OUTSIDE_ROOM,
                            dt_ms,
                            ..Default::default()
                        },
                        None,
                    )
                }
                _ => {
                    let v = velocity.length() as f32;
                    ik_activity = if p.remote {
                        p.activity
                    } else {
                        Activity::Stand
                    };
                    let seat = p
                        .render
                        .mirror_seat
                        .and_then(|k| bn.and_then(|b| b.cabin.seats.get(k)));
                    let waiting_seat = self
                        .network
                        .mirror_wait
                        .get(&p.id)
                        .and_then(|(s, k)| self.stops.get(s).and_then(|s| s.spots.get(*k)))
                        .filter(|sp| sp.height != 0.0);
                    if let Some(seat) = seat.filter(|seat| seat.seated) {
                        ik_seat = Some(to_model(seat.pos.as_dvec3()));
                        facing = Some(seat.rot as f64);
                    }
                    if let Some(sp) = waiting_seat {
                        ik_seat = Some(to_model(sp.pos));
                        facing = Some(sp.face);
                    }
                    (
                        AnimInput {
                            kind: if p.activity == Activity::Sit && ik_seat.is_some() {
                                2
                            } else if v > 0.05 {
                                1
                            } else {
                                0
                            },
                            seat_height: seat
                                .map(|s| s.height)
                                .or_else(|| waiting_seat.map(|s| s.height))
                                .unwrap_or(0.0),
                            speed: v,
                            moved: v * dt,
                            room_height: pax::OUTSIDE_ROOM,
                            dt_ms,
                            ..Default::default()
                        },
                        None,
                    )
                }
            };
            if ik_activity == Activity::Sit {
                let aligned = facing
                    .map(|f| crowd::angle_diff(heading, f).abs() < 30.0)
                    .unwrap_or(true);
                if ik_seat.is_none() || (!aligned && p.pose.sit_amount() < 0.3) {
                    ik_activity = Activity::Stand;
                    ik_seat = None;
                }
            }
            let sway = match bn {
                Some(b) => model_point(DVec3::ZERO, heading, b.accel.extend(0.0)),
                None => Vec3::ZERO,
            };
            let p = &mut self.people[i];
            let is_bus = matches!(p.place, Place::Bus(..));
            let floor_cb = |at: DVec2| -> Option<f64> {
                if is_bus {
                    Some(origin.z)
                } else {
                    world
                        .walk_height_near(at.x, at.y, origin.z)
                        .filter(|&z| (z - origin.z).abs() < 0.3)
                        .or(Some(origin.z))
                }
            };
            let pose_input = omsi_sim::human::PoseInput {
                activity: ik_activity,
                origin,
                heading,
                frame,
                velocity,
                seat: ik_seat,
                look: ik_look,
                reach: ik_reach,
                grips: None,
                grip_frames: None,
                grip_lean: 0.0,
                hold: ik_hold,
                sway: sway.truncate(),
                floor: Some(&floor_cb),
            };
            p.pose.advance(&p.ty.rig, &pose_input, dt);
            p.activity = ik_activity;
            let step = p.finish_animation(self.ik, &input);
            // a foot down inside a vehicle: the link's step sound (outside there are none)
            if step && footstep.is_some() {
                if let Some((bus, pack)) = footstep {
                    self.footfalls.push(ambience::Footfall {
                        position: p.position,
                        inside: true,
                        own_bus: bus == BusId::Player,
                        pack: Some(pack),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::active_footstep;

    #[test]
    fn only_the_active_animation_or_its_actual_invalid_pose_fallback_can_step() {
        assert!(!active_footstep(true, true, false, true));
        assert!(active_footstep(true, true, true, false));
        assert!(!active_footstep(false, true, true, false));
        assert!(active_footstep(false, true, false, true));
        assert!(active_footstep(true, false, false, true));
        assert!(!active_footstep(true, false, true, false));
    }
}
