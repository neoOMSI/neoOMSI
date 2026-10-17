use super::*;

pub(crate) fn steering_view_yaw(
    current: f32,
    steering: f32,
    dt: f32,
    enabled: bool,
    angle: f32,
    response: f32,
) -> f32 {
    let target = if enabled {
        steering.clamp(-1.0, 1.0) * angle.clamp(0.0, 60.0)
    } else {
        0.0
    };
    current + (target - current) * (1.0 - (-dt.max(0.0) / response.clamp(0.05, 1.0)).exp())
}

impl Player {
    pub(crate) fn sync_transforms(&mut self, renderer: &Renderer, scene: &mut Scene, inside: bool) {
        sync_vehicle_transforms(
            renderer,
            scene,
            &mut self.vehicle,
            &mut self.render,
            &mut self.trailer_renders,
            inside,
        );
        if self.render.window_wipers.is_none() {
            self.render.window_wipers = Some(window_wipers::WindowWipers::new(
                renderer,
                scene,
                &self.vehicle,
                &self.render,
            ));
        }
        let wipers = self.render.window_wipers.as_mut().unwrap();
        wipers.update(renderer, scene, &self.vehicle, &self.render.instances);
    }

    pub(crate) fn sync_driver(
        &mut self,
        renderer: &Renderer,
        scene: &mut Scene,
        dt: f32,
        show: bool,
        mirror_only: bool,
    ) {
        self.sync_driver_hands(renderer, scene, dt, show, mirror_only, false);
    }

    pub(crate) fn sync_driver_hands(
        &mut self,
        renderer: &Renderer,
        scene: &mut Scene,
        dt: f32,
        show: bool,
        mirror_only: bool,
        hands: bool,
    ) {
        if let Some(d) = self.driver.as_mut() {
            d.show_hands_in_cab = hands;
            d.update(
                renderer,
                scene,
                &self.vehicle,
                &self.render,
                dt,
                show,
                mirror_only,
            );
        }
    }

    pub(crate) fn driver_local(&self, look: (f32, f32)) -> Option<::legacy_vehicle::Camera> {
        if self.trailer_driver_camera().is_some() {
            return None;
        }
        let def = &self.vehicle.ty.def;
        let n = def.cameras_driver.len().max(1);
        let c = def
            .cameras_driver
            .get((def.camera_std + self.cam_choice.0) % n)
            .or(def.cameras_driver.first())?;
        Some(::legacy_vehicle::Camera {
            yaw: c.yaw + look.0 + self.steer_look,
            pitch: self.driver_pitch(c.pitch, look.1),
            ..c.clone()
        })
    }

    pub(super) fn driver_pitch(&self, camera_pitch: f32, look_pitch: f32) -> f32 {
        driver_pitch(
            camera_pitch,
            look_pitch,
            (::config::get_float("camera", "head_pitch").unwrap_or(0.0) as f32).clamp(-45.0, 45.0),
        )
    }

    pub(crate) fn driver_world(&self, turned: &::legacy_vehicle::Camera) -> Camera {
        let (eye, yaw, pitch, roll) = self.vehicle.camera_world_full(turned);
        let eye = eye
            + self
            .vehicle
            .body_rotation()
            .transform_vector3(self.head + self.seat)
            .as_dvec3();
        Camera {
            position: eye,
            yaw,
            pitch: pitch.clamp(-89.0, 89.0),
            roll,
            fov_deg: turned.fov,
            near: 0.1,
            far: 6000.0,
        }
    }

    pub(crate) fn local_of_world(&self, cam: &Camera) -> ::legacy_vehicle::Camera {
        let inv = self.vehicle.body_rotation().inverse();
        let pos = inv.transform_vector3((cam.position - self.vehicle.position).as_vec3());
        let f = inv.transform_vector3(cam.forward());
        ::legacy_vehicle::Camera {
            pos: pos.to_array(),
            yaw: f.x.atan2(f.y).to_degrees(),
            pitch: f.z.clamp(-1.0, 1.0).asin().to_degrees(),
            fov: cam.fov_deg,
            ..Default::default()
        }
    }

    pub(crate) fn camera(&self, view: &str, fallback: &Camera) -> Camera {
        self.camera_look(view, fallback, (0.0, 0.0), ORBIT_DEFAULT)
    }

    pub(crate) fn camera_clipped(
        &mut self,
        mut cam: Camera,
        world: &World,
        dist: f32,
        dt: f32,
    ) -> Camera {
        let def = &self.vehicle.ty.def;
        let c = def.camera_outside_center;
        let centre = orbit_pivot(self.vehicle.position, self.vehicle.heading, c);
        let want = dist.clamp(ORBIT_MIN, ORBIT_MAX);
        let back = -cam.forward().as_dvec3().normalize_or_zero();
        if back.length_squared() < 0.5 {
            return cam;
        }
        let right = cam.right().as_dvec3().normalize_or_zero();
        let up = back.cross(right).normalize_or_zero();
        let t0 = Instant::now();
        let free = camera_arm::free_length(world, centre, back, right, up, want as f64) as f32;
        let len = if dt <= 0.0 {
            self.arm.reset();
            free
        } else {
            self.arm.update(want, free, centre, dt)
        };
        if camera_arm::debug_level() >= 2 {
            log::info!(
                "camera arm: yaw {:.1} want {want:.2} free {free:.2} arm {len:.2} ({:.3} ms)",
                cam.yaw,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        cam.position = centre + back * len as f64;
        cam
    }

    pub(crate) fn pax_camera_count(&self) -> usize {
        self.vehicle.ty.def.cameras_pax.len()
            + self
            .vehicle
            .trailers
            .iter()
            .map(|t| t.ty.def.cameras_pax.len())
            .sum::<usize>()
    }

    pub(crate) fn driver_camera_count(&self) -> usize {
        self.vehicle.ty.def.cameras_driver.len()
            + self
            .vehicle
            .trailers
            .iter()
            .map(|t| t.ty.def.cameras_driver.len())
            .sum::<usize>()
    }

    pub(crate) fn trailer_driver_camera(
        &self,
    ) -> Option<(&::simulation::vehicle::TrailerPart, &::legacy_vehicle::Camera)> {
        let front = self.vehicle.ty.def.cameras_driver.len();
        let mut k = self.cam_choice.0 % self.driver_camera_count().max(1);
        if k < front {
            return None;
        }
        k -= front;
        for t in &self.vehicle.trailers {
            if let Some(c) = t.ty.def.cameras_driver.get(k) {
                return Some((t, c));
            }
            k -= t.ty.def.cameras_driver.len();
        }
        None
    }

    pub(crate) fn camera_look(
        &self,
        view: &str,
        fallback: &Camera,
        look: (f32, f32),
        dist: f32,
    ) -> Camera {
        let def = &self.vehicle.ty.def;
        if let Some(c) = view
            .strip_prefix("mirror")
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| mirror_cams(&self.vehicle).into_iter().nth(n).map(|(t, c)| (n, t, c)))
        {
            let (k, part, c) = c;
            let aimed = aim_camera(
                &self.vehicle,
                part,
                k,
                c,
                driver_eye(self),
                self.mirror_offsets.get(k).copied().unwrap_or([0.0; 2]),
            );
            let (eye, yaw, pitch, roll) = mirror_cam_world_full(&self.vehicle, part, &aimed);
            return Camera {
                position: eye,
                yaw,
                pitch,
                roll,
                fov_deg: if c.fov > 1.0 { c.fov } else { 50.0 },
                near: 0.1,
                far: 450.0,
            };
        }
        let cam = match view {
            "driver" => {
                if let Some((t, c)) = self.trailer_driver_camera() {
                    let turned = ::legacy_vehicle::Camera {
                        yaw: c.yaw + look.0 + self.steer_look,
                        pitch: self.driver_pitch(c.pitch, look.1),
                        ..c.clone()
                    };
                    let (eye, yaw, pitch, roll) = t.camera_world_full(&turned);
                    return Camera {
                        position: eye,
                        yaw,
                        pitch: pitch.clamp(-89.0, 89.0),
                        roll,
                        fov_deg: c.fov,
                        near: 0.1,
                        far: 6000.0,
                    };
                }
                let n = def.cameras_driver.len().max(1);
                def.cameras_driver
                    .get((def.camera_std + self.cam_choice.0) % n)
                    .or(def.cameras_driver.first())
            }
            "pax" => {
                let n = self.pax_camera_count().max(1);
                let k = self.cam_choice.1 % n;
                match def.cameras_pax.get(k) {
                    Some(c) => Some(c),
                    None => {
                        let mut k = k - def.cameras_pax.len();
                        let mut found = None;
                        for t in &self.vehicle.trailers {
                            if let Some(c) = t.ty.def.cameras_pax.get(k) {
                                let turned = ::legacy_vehicle::Camera {
                                    yaw: c.yaw + look.0,
                                    pitch: (c.pitch + look.1).clamp(-89.0, 89.0),
                                    ..c.clone()
                                };
                                found = Some((t.camera_world_full(&turned), c.fov));
                                break;
                            }
                            k -= t.ty.def.cameras_pax.len();
                        }
                        if let Some(((eye, yaw, pitch, roll), fov)) = found {
                            return Camera {
                                position: eye,
                                yaw,
                                pitch: pitch.clamp(-89.0, 89.0),
                                roll,
                                fov_deg: fov,
                                near: 0.1,
                                far: 6000.0,
                            };
                        }
                        def.cameras_pax.first()
                    }
                }
            }
            _ => None,
        };
        match cam {
            Some(c) => {
                let turned = ::legacy_vehicle::Camera {
                    yaw: c.yaw
                        + look.0
                        + if view == "driver" {
                        self.steer_look
                    } else {
                        0.0
                    },
                    pitch: if view == "driver" {
                        self.driver_pitch(c.pitch, look.1)
                    } else {
                        (c.pitch + look.1).clamp(-89.0, 89.0)
                    },
                    ..c.clone()
                };
                let (eye, yaw, pitch, roll) = self.vehicle.camera_world_full(&turned);
                let eye = if view == "driver" {
                    eye + self
                        .vehicle
                        .body_rotation()
                        .transform_vector3(self.head + self.seat)
                        .as_dvec3()
                } else {
                    eye
                };
                Camera {
                    position: eye,
                    yaw,
                    pitch: pitch.clamp(-89.0, 89.0),
                    roll,
                    fov_deg: c.fov,
                    near: 0.1,
                    far: 6000.0,
                }
            }
            None => {
                let c = def.camera_outside_center;
                let center = orbit_pivot(self.vehicle.position, self.vehicle.heading, c);
                let mut cam = Camera {
                    position: center,
                    yaw: self.vehicle.heading as f32 - 35.0 + look.0,
                    pitch: (-15.0 + look.1).clamp(-85.0, 85.0),
                    roll: 0.0,
                    fov_deg: fallback.fov_deg,
                    near: 0.1,
                    far: 6000.0,
                };
                cam.position =
                    center - (cam.forward() * dist.clamp(ORBIT_MIN, ORBIT_MAX)).as_dvec3();
                cam
            }
        }
    }
}

pub(crate) fn orbit_pivot(position: DVec3, heading_deg: f64, center: [f32; 3]) -> DVec3 {
    position
        + glam::Mat4::from_rotation_z((-(heading_deg as f32)).to_radians())
        .transform_point3(Vec3::new(center[0], center[1], center[2]))
        .as_dvec3()
}

pub(super) fn driver_pitch(camera_pitch: f32, look_pitch: f32, head_pitch: f32) -> f32 {
    (camera_pitch + head_pitch + look_pitch).clamp(-89.0, 89.0)
}

#[cfg(test)]
mod driver_pitch_tests {
    use super::driver_pitch;

    #[test]
    fn offset_is_added_to_the_configured_pitch_before_manual_look() {
        assert_eq!(driver_pitch(12.0, 0.0, 0.0), 12.0);
        assert_eq!(driver_pitch(12.0, 3.0, 8.0), 23.0);
        assert_eq!(driver_pitch(-12.0, -3.0, -8.0), -23.0);
    }

    #[test]
    fn driver_pitch_keeps_the_existing_limits() {
        assert_eq!(driver_pitch(80.0, 20.0, 45.0), 89.0);
        assert_eq!(driver_pitch(-80.0, -20.0, -45.0), -89.0);
    }
}

pub(crate) fn sync_vehicle_transforms(
    renderer: &Renderer,
    scene: &mut Scene,
    vehicle: &mut ::simulation::VehicleInstance,
    render: &mut scene::VehicleRender,
    trailer_renders: &mut [scene::VehicleRender],
    inside: bool,
) {
    scene::sync_vehicle_materials(renderer, scene, vehicle, render);
    scene::sync_skinned(renderer, scene, vehicle, render, trailer_renders);
    for (i, inst) in render.instances.iter().enumerate() {
        renderer.set_transform(
            scene,
            *inst,
            vehicle.position,
            vehicle.mesh_local_transform(i),
        );
        let p = &vehicle.mesh_props[i];
        let def = &vehicle.ty.model.meshes[vehicle.ty.meshes[i].def_index];
        let vp = def.viewpoint;
        let vp_ok = vp == 0 || (inside && vp & 2 != 0) || (!inside && vp & 1 != 0);
        let mirror = inside && vp & 1 != 0 && vp & 2 == 0;
        let mut alpha = p.slot_alpha.clone();
        for (slot, mat) in scene.instances[*inst].materials.iter().enumerate() {
            if scene
                .materials
                .get(*mat)
                .is_some_and(|m| m.alpha == ::render::AlphaMode::Opaque)
            {
                if let Some(a) = alpha.get_mut(slot) {
                    *a = 1.0;
                }
            }
        }
        renderer.set_params(
            scene,
            *inst,
            &alpha,
            p.visible && (vp_ok || mirror),
            &p.slot_uv,
        );
        renderer.set_mirror_only(scene, *inst, mirror);
        renderer.set_slot_light(scene, *inst, &p.slot_light);
        renderer.set_slot_night(scene, *inst, &p.slot_night);
        renderer.set_interior(scene, *inst, p.interior);
        renderer.set_cabin(scene, *inst, !def.illumination_interior.is_empty());
    }
    for (t, r) in vehicle.trailers.iter().zip(trailer_renders.iter()) {
        for (i, inst) in r.instances.iter().enumerate() {
            renderer.set_transform(scene, *inst, t.position, t.mesh_local_transform(i));
            let p = &t.mesh_props[i];
            let def = &t.ty.model.meshes[t.ty.meshes[i].def_index];
            let vp = def.viewpoint;
            let vp_ok = vp == 0 || (inside && vp & 2 != 0) || (!inside && vp & 1 != 0);
            let mirror = inside && vp & 1 != 0 && vp & 2 == 0;
            let mut alpha = p.slot_alpha.clone();
            for (slot, mat) in scene.instances[*inst].materials.iter().enumerate() {
                if scene
                    .materials
                    .get(*mat)
                    .is_some_and(|m| m.alpha == ::render::AlphaMode::Opaque)
                {
                    if let Some(a) = alpha.get_mut(slot) {
                        *a = 1.0;
                    }
                }
            }
            renderer.set_params(
                scene,
                *inst,
                &alpha,
                p.visible && (vp_ok || mirror),
                &p.slot_uv,
            );
            renderer.set_mirror_only(scene, *inst, mirror);
            renderer.set_slot_light(scene, *inst, &p.slot_light);
            renderer.set_slot_night(scene, *inst, &p.slot_night);
            renderer.set_interior(scene, *inst, p.interior);
            renderer.set_cabin(scene, *inst, !def.illumination_interior.is_empty());
        }
    }
    scene::sync_vehicle_textures(renderer, scene, vehicle, render, &mut { usize::MAX });
    let mut trailers = std::mem::take(&mut vehicle.trailers);
    for (t, r) in trailers.iter_mut().zip(trailer_renders.iter_mut()) {
        scene::sync_vehicle_part(renderer, scene, &vehicle, t, r);
    }
    vehicle.trailers = trailers;
}

#[cfg(test)]
mod orbit_pivot_tests {
    use super::orbit_pivot;
    use glam::DVec3;

    #[test]
    fn pivot_yaws_with_the_bus_and_ignores_body_attitude() {
        let p = orbit_pivot(DVec3::new(10.0, 20.0, 5.0), 0.0, [0.0, 0.0, 1.2]);
        assert!((p.x - 10.0).abs() < 1e-6 && (p.y - 20.0).abs() < 1e-6);
        assert!((p.z - 6.2).abs() < 1e-6, "{p:?}");
        let q = orbit_pivot(DVec3::ZERO, 90.0, [1.0, 2.0, 1.2]);
        assert!(
            (q.x - 2.0).abs() < 1e-5 && (q.y + 1.0).abs() < 1e-5,
            "{q:?}"
        );
        assert!((q.z - 1.2).abs() < 1e-6, "{q:?}");
    }
}

#[cfg(test)]
mod steering_view_tests {
    use super::steering_view_yaw;

    #[test]
    fn follows_both_directions_without_snapping_or_overshooting() {
        let right = steering_view_yaw(0.0, 1.0, 0.016, true, 30.0, 0.25);
        assert!(right > 0.0 && right < 30.0);
        assert_eq!(
            steering_view_yaw(0.0, -1.0, 0.016, true, 30.0, 0.25),
            -right
        );
        let changed = steering_view_yaw(30.0, -1.0, 0.016, true, 30.0, 0.25);
        assert!(changed < 30.0 && changed > -30.0);
    }

    #[test]
    fn smoothing_is_frame_rate_independent_even_at_five_fps() {
        let mut reference: Option<f32> = None;
        for fps in [5, 30, 60, 144] {
            let mut yaw = 0.0;
            for _ in 0..fps {
                yaw = steering_view_yaw(yaw, 0.8, 1.0 / fps as f32, true, 40.0, 0.25);
            }
            if let Some(reference) = reference {
                assert!((yaw - reference).abs() < 0.0001);
            }
            reference = Some(yaw);
        }
    }

    #[test]
    fn configurable_angle_response_and_return_to_center() {
        let slow = steering_view_yaw(0.0, 1.0, 0.1, true, 45.0, 0.5);
        let fast = steering_view_yaw(0.0, 1.0, 0.1, true, 45.0, 0.1);
        assert!(fast > slow);
        assert_eq!(steering_view_yaw(0.0, 2.0, 0.1, true, 45.0, 0.1), fast);
        let centered = steering_view_yaw(30.0, 1.0, 0.1, false, 45.0, 0.25);
        assert!(centered > 0.0 && centered < 30.0);
        assert_eq!(steering_view_yaw(30.0, 0.0, 0.0, true, 45.0, 0.25), 30.0);
    }
}
