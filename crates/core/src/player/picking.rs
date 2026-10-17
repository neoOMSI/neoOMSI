use super::*;

impl Player {
    pub(crate) fn pick(&self, origin: DVec3, dir: Vec3, spread: f32) -> Option<usize> {
        let (i, hit_dir) = pick_in(&self.vehicle, origin, dir, spread)?;
        if self.control_hidden(origin, hit_dir, None, i) {
            return None;
        }
        Some(i)
    }

    pub(crate) fn html_hit(&self, origin: DVec3, dir: Vec3) -> Option<(usize, f32, f32)> {
        pick_html_in(&self.vehicle, origin, dir)
    }

    pub(crate) fn html_pointer(
        &mut self,
        page: usize,
        u: f32,
        v: f32,
        kind: ::simulation::htmltex::PointerKind,
    ) -> bool {
        self.vehicle.html_pointer(page, u, v, kind)
    }

    pub(crate) fn pick_trailer(
        &self,
        origin: DVec3,
        dir: Vec3,
        spread: f32,
    ) -> Option<(usize, usize)> {
        let ((ti, i), hit_dir) = pick_trailer_in(&self.vehicle, origin, dir, spread)?;
        if self.control_hidden(origin, hit_dir, Some(ti), i) {
            return None;
        }
        Some((ti, i))
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn surface_hit(&self, origin: DVec3, dir: Vec3) -> Option<DVec3> {
        let (mut nearest, nearest_control) = self.nearest_hits(origin, dir, None);
        if nearest_control.is_finite() {
            nearest = nearest_control;
        }
        (nearest.is_finite() && nearest < 8.0).then(|| origin + (dir * nearest).as_dvec3())
    }

    pub(crate) fn body_hit(&self, origin: DVec3, dir: Vec3) -> Option<f32> {
        Some(self.nearest_hits(origin, dir, None).0).filter(|t| t.is_finite())
    }

    pub(super) fn control_hidden(&self, origin: DVec3, dir: Vec3, trailer: Option<usize>, i: usize) -> bool {
        let (ty, position, xf) = match trailer {
            None => (
                &self.vehicle.ty,
                self.vehicle.position,
                self.vehicle.mesh_local_transform(i),
            ),
            Some(ti) => {
                let t = &self.vehicle.trailers[ti];
                (&t.ty, t.position, t.mesh_local_transform(i))
            }
        };
        let dir = dir.normalize_or_zero();
        let o = (origin - position).as_vec3();
        if let Some(t) = ::geometry::ray_mesh(o, dir, &ty.meshes[i].data, &xf) {
            let other = self.nearest_hits(origin, dir, Some((trailer, i))).0;
            return in_front_of(other, t);
        }
        let Some(&(c, r)) = ty.mesh_bounds.get(i) else {
            return false;
        };
        let scale = xf
            .x_axis
            .truncate()
            .length()
            .max(xf.y_axis.truncate().length())
            .max(xf.z_axis.truncate().length());
        let along = (xf.transform_point3(c) - o).dot(dir);
        let nearest = self.nearest_hits(origin, dir, Some((trailer, i))).0;
        nearest < along - r * scale - 0.1
    }

    pub(super) fn nearest_hits(
        &self,
        origin: DVec3,
        dir: Vec3,
        skip: Option<(Option<usize>, usize)>,
    ) -> (f32, f32) {
        let mut nearest = f32::INFINITY;
        let mut nearest_control = f32::INFINITY;
        let vehicle = &self.vehicle;
        let o = (origin - vehicle.position).as_vec3();
        for (i, mesh) in vehicle.ty.meshes.iter().enumerate() {
            if !vehicle.mesh_props[i].visible || skip == Some((None, i)) {
                continue;
            }
            let transform = vehicle.mesh_local_transform(i);
            if !ray_may_hit(&vehicle.ty, i, &transform, o, dir, 0.0) {
                continue;
            }
            if encloses_origin(&vehicle.ty, i, &transform, o, dir) {
                continue;
            }
            if let Some(t) = ::geometry::ray_mesh(o, dir, &mesh.data, &transform) {
                if t > 0.02 {
                    if t < nearest {
                        nearest = t;
                    }
                    if vehicle.ty.model.meshes[mesh.def_index]
                        .mouse_event
                        .is_some()
                        && t < nearest_control
                    {
                        nearest_control = t;
                    }
                }
            }
        }
        for (ti, trailer) in vehicle.trailers.iter().enumerate() {
            let o = (origin - trailer.position).as_vec3();
            for (i, mesh) in trailer.ty.meshes.iter().enumerate() {
                if !trailer.mesh_props[i].visible || skip == Some((Some(ti), i)) {
                    continue;
                }
                let transform = trailer.mesh_local_transform(i);
                if !ray_may_hit(&trailer.ty, i, &transform, o, dir, 0.0) {
                    continue;
                }
                if encloses_origin(&trailer.ty, i, &transform, o, dir) {
                    continue;
                }
                if let Some(t) = ::geometry::ray_mesh(o, dir, &mesh.data, &transform) {
                    if t > 0.02 {
                        if t < nearest {
                            nearest = t;
                        }
                        if trailer.ty.model.meshes[mesh.def_index]
                            .mouse_event
                            .is_some()
                            && t < nearest_control
                        {
                            nearest_control = t;
                        }
                    }
                }
            }
        }
        (nearest, nearest_control)
    }

    pub(crate) fn hovered_part(
        &self,
        origin: DVec3,
        dir: Vec3,
        spread: f32,
    ) -> (Option<(String, bool)>, bool) {
        if let Some(i) = self.pick(origin, dir, spread) {
            let def = &self.vehicle.ty.model.meshes[self.vehicle.ty.meshes[i].def_index];
            if let Some(ev) = def.mouse_event.clone() {
                let big = self
                    .vehicle
                    .ty
                    .mesh_bounds
                    .get(i)
                    .map(|b| b.1 > 0.45)
                    .unwrap_or(false);
                if big {
                    return (None, true);
                }
                return (Some((ev, true)), true);
            }
        }
        if let Some((ti, i)) = self.pick_trailer(origin, dir, spread) {
            let trailer = &self.vehicle.trailers[ti];
            let def = &trailer.ty.model.meshes[trailer.ty.meshes[i].def_index];
            if let Some(ev) = def.mouse_event.clone() {
                return (Some((ev, true)), true);
            }
        }
        (self.hovered_body_part(origin, dir), false)
    }

    pub(super) fn hovered_body_part(&self, origin: DVec3, dir: Vec3) -> Option<(String, bool)> {
        let o = (origin - self.vehicle.position).as_vec3();
        let mut best: Option<(f32, usize)> = None;
        for (i, vm) in self.vehicle.ty.meshes.iter().enumerate() {
            if !self.vehicle.mesh_props[i].visible {
                continue;
            }
            let xf = self.vehicle.mesh_local_transform(i);
            if !ray_may_hit(&self.vehicle.ty, i, &xf, o, dir, 0.0) {
                continue;
            }
            if let Some(t) = ::geometry::ray_mesh(o, dir, &vm.data, &xf) {
                if best.map(|(bt, _)| t < bt).unwrap_or(true) {
                    best = Some((t, i));
                }
            }
        }
        let (_, i) = best?;
        let def = &self.vehicle.ty.model.meshes[self.vehicle.ty.meshes[i].def_index];
        let name = Path::new(&def.file.replace('\\', "/"))
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| def.file.clone());
        Some((name, false))
    }

    pub(crate) fn click(&mut self, origin: DVec3, dir: Vec3, spread: f32) -> Option<usize> {
        self.release();
        if let Some(i) = self.pick(origin, dir, spread) {
            let ev = self.vehicle.ty.model.meshes[self.vehicle.ty.meshes[i].def_index]
                .mouse_event
                .clone()
                .unwrap();
            log::info!("mouse event {ev}");
            let plain = self.vehicle.trigger(&ev);
            self.repair_roller_blind(&ev);
            self.pressed_mesh = Some(i);
            self.press_info = (plain, 0.0);
            self.auto_drag = None;
            return Some(i);
        }
        let (ti, i) = self.pick_trailer(origin, dir, spread)?;
        let ev = self.vehicle.trailers[ti].ty.model.meshes
            [self.vehicle.trailers[ti].ty.meshes[i].def_index]
            .mouse_event
            .clone()
            .unwrap();
        log::info!("trailer mouse event {ev} (part {ti})");
        self.vehicle.trigger(&ev);
        self.repair_roller_blind(&ev);
        self.pressed_trailer_mesh = Some((ti, i));
        Some(i)
    }

    pub(crate) fn drag(&mut self, dx: f32, dy: f32) {
        let (ev, trailer) = if let Some(i) = self.pressed_mesh {
            let Some(ev) = self.vehicle.ty.model.meshes[self.vehicle.ty.meshes[i].def_index]
                .mouse_event
                .clone()
            else {
                return;
            };
            (ev, false)
        } else if let Some((ti, i)) = self.pressed_trailer_mesh {
            let Some(ev) = self.vehicle.trailers[ti].ty.model.meshes
                [self.vehicle.trailers[ti].ty.meshes[i].def_index]
                .mouse_event
                .clone()
            else {
                return;
            };
            (ev, true)
        } else {
            return;
        };
        let name = format!("{ev}_drag");
        self.press_info.1 += dx.abs() + dy.abs();
        self.vehicle.host.mouse = (dx, dy);
        self.vehicle.trigger(&name);
        self.repair_roller_blind(&name);
        self.vehicle.host.mouse = (0.0, 0.0);
        let _ = trailer;
    }

    pub(crate) fn wheel(&mut self, origin: DVec3, dir: Vec3, spread: f32, amount: f32) {
        let Some(i) = self.pick(origin, dir, spread) else {
            return;
        };
        let Some(ev) = self.vehicle.ty.model.meshes[self.vehicle.ty.meshes[i].def_index]
            .mouse_event
            .clone()
        else {
            return;
        };
        self.vehicle.host.mouse = (0.0, amount);
        self.vehicle.trigger(&format!("{ev}_drag"));
        self.repair_roller_blind(&format!("{ev}_drag"));
        self.vehicle.host.mouse = (0.0, 0.0);
        self.vehicle.trigger(&format!("{ev}_off"));
    }

    pub(crate) fn release_keeping(&mut self) {
        if let Some(i) = self.pressed_mesh.take() {
            let def = &self.vehicle.ty.model.meshes[self.vehicle.ty.meshes[i].def_index];
            log::info!(
                "mouse event {:?}: let go with the right button held, the switch stays",
                def.mouse_event
            );
        }
        self.pressed_trailer_mesh = None;
    }

    pub(crate) fn release(&mut self) {
        if let Some(i) = self.pressed_mesh.take() {
            let ty = self.vehicle.ty.clone();
            let def = &ty.model.meshes[ty.meshes[i].def_index];
            if let Some(ev) = def.mouse_event.clone() {
                self.vehicle.trigger(&format!("{ev}_off"));
                let anim = def
                    .animations
                    .first()
                    .map(|a| a.variable.clone())
                    .filter(|v| !v.trim().is_empty() && v.trim().parse::<f32>().is_err());
                if let (false, true, Some(var)) = (self.press_info.0, self.press_info.1 < 4.0, anim)
                {
                    let now = self.vehicle.var(&var).unwrap_or(0.0);
                    let target = if now > 0.5 { 0.0 } else { 1.0 };
                    log::info!(
                        "mouse event {ev}: a click on a drag control, {var} {now:.2} -> {target}"
                    );
                    self.auto_drag = Some(AutoDrag {
                        ev,
                        var,
                        target,
                        axis: 0,
                        sign: if target > now { -1.0 } else { 1.0 },
                        last: now,
                        stalled: 0,
                        t: 0.0,
                    });
                }
            }
        }
        if let Some((ti, i)) = self.pressed_trailer_mesh.take() {
            if let Some(ev) = self.vehicle.trailers[ti].ty.model.meshes
                [self.vehicle.trailers[ti].ty.meshes[i].def_index]
                .mouse_event
                .clone()
            {
                self.vehicle.trigger(&format!("{ev}_off"));
            }
        }
    }

    pub(crate) fn tick_auto_drag(&mut self, dt: f32) {
        let Some(mut a) = self.auto_drag.take() else {
            return;
        };
        let now = self.vehicle.var(&a.var).unwrap_or(a.last);
        a.t += dt;
        if (now - a.target).abs() < 0.03 || a.t > 2.5 {
            self.vehicle.trigger(&format!("{}_off", a.ev));
            return;
        }
        let towards = (a.target - a.last).signum();
        let moved = now - a.last;
        if a.t > 0.05 {
            if moved * towards < -1e-4 {
                a.sign = -a.sign;
            } else if moved.abs() < 1e-4 {
                a.stalled += 1;
                if a.stalled == 3 {
                    a.sign = -a.sign;
                } else if a.stalled == 6 {
                    a.axis = 1 - a.axis;
                } else if a.stalled == 9 {
                    a.sign = -a.sign;
                } else if a.stalled > 12 {
                    self.vehicle.trigger(&format!("{}_off", a.ev));
                    return;
                }
            } else {
                a.stalled = 0;
            }
        }
        a.last = now;
        let step = 900.0 * dt.min(0.05) * a.sign;
        self.vehicle.host.mouse = if a.axis == 0 {
            (step, 0.0)
        } else {
            (0.0, step)
        };
        let exists = self.vehicle.trigger(&format!("{}_drag", a.ev));
        self.vehicle.host.mouse = (0.0, 0.0);
        if exists {
            self.auto_drag = Some(a);
        }
    }
}

pub(crate) fn pick_in(
    vehicle: &::simulation::VehicleInstance,
    origin: DVec3,
    dir: Vec3,
    spread: f32,
) -> Option<(usize, Vec3)> {
    let o = (origin - vehicle.position).as_vec3();
    let right = Vec3::new(-dir.y, dir.x, 0.0).normalize_or_zero();
    let up = dir.cross(right).normalize_or_zero();
    let mut rings: Vec<Vec<Vec3>> = vec![vec![dir]];
    if spread > 0.0 {
        for ring in 1..=2 {
            let r = spread * ring as f32;
            let n = 8 * ring;
            rings.push(
                (0..n)
                    .map(|k| {
                        let a = k as f32 / n as f32 * std::f32::consts::TAU;
                        (dir + right * (a.cos() * r) + up * (a.sin() * r)).normalize()
                    })
                    .collect(),
            );
        }
    }
    let candidates: Vec<(usize, glam::Mat4, Vec<u32>)> = vehicle
        .ty
        .meshes
        .iter()
        .enumerate()
        .filter_map(|(i, vm)| {
            if vehicle.ty.model.meshes[vm.def_index].mouse_event.is_none()
                || !vehicle.mesh_props[i].visible
            {
                return None;
            }
            let xf = vehicle.mesh_local_transform(i);
            if !ray_may_hit(&vehicle.ty, i, &xf, o, dir, spread * 2.2) {
                return None;
            }
            let tris = ::geometry::cone_triangles(o, dir, spread * 2.0 + 1e-4, &vm.data, &xf);
            (!tris.is_empty()).then_some((i, xf, tris))
        })
        .collect();
    for dirs in &rings {
        let mut best: Option<(f32, usize, Vec3)> = None;
        for (i, xf, tris) in &candidates {
            let (i, xf) = (*i, *xf);
            let vm = &vehicle.ty.meshes[i];
            for d in dirs {
                if let Some(t) = ::geometry::ray_triangles(o, *d, &vm.data, &xf, tris) {
                    if best.map(|(bt, _, _)| t < bt).unwrap_or(true) {
                        best = Some((t, i, *d));
                    }
                }
            }
        }
        if let Some((_, i, d)) = best {
            return Some((i, d));
        }
    }
    None
}

pub(crate) fn pick_html_in(
    vehicle: &::simulation::VehicleInstance,
    origin: DVec3,
    dir: Vec3,
) -> Option<(usize, f32, f32)> {
    if vehicle.html_textures.is_empty() {
        return None;
    }
    let pages: Vec<usize> = vehicle
        .html_textures
        .iter()
        .map(|t| t.script_index)
        .collect();
    let o = (origin - vehicle.position).as_vec3();
    let mut best: Option<(f32, usize, f32, f32)> = None;
    for (i, vm) in vehicle.ty.meshes.iter().enumerate() {
        if !vehicle.mesh_props[i].visible {
            continue;
        }
        let def = &vehicle.ty.model.meshes[vm.def_index];
        let shows_page = |n: Option<i32>| n.is_some_and(|n| pages.contains(&(n.max(0) as usize)));
        if !def
            .materials
            .iter()
            .any(|m| shows_page(m.use_script_texture))
        {
            continue;
        }
        let xf = vehicle.mesh_local_transform(i);
        if !ray_may_hit(&vehicle.ty, i, &xf, o, dir, 0.0) {
            continue;
        }
        let Some(hit) = ::geometry::ray_mesh_hit(o, dir, &vm.data, &xf) else {
            continue;
        };
        let slot = vm.data.slot_of(hit.index) as usize;
        let page = def
            .materials
            .iter()
            .filter(|m| ::simulation::vehicle::override_slot(&vm.materials, m) == Some(slot))
            .find_map(|m| m.use_script_texture)
            .map(|n| n.max(0) as usize)
            .filter(|n| pages.contains(n));
        let Some(page) = page else { continue };
        if best.is_none_or(|b| hit.t < b.0) {
            best = Some((
                hit.t,
                page,
                hit.uv.x.clamp(0.0, 1.0),
                hit.uv.y.clamp(0.0, 1.0),
            ));
        }
    }
    best.map(|(_, page, u, v)| (page, u, v))
}

pub(crate) fn pick_trailer_in(
    vehicle: &::simulation::VehicleInstance,
    origin: DVec3,
    dir: Vec3,
    spread: f32,
) -> Option<((usize, usize), Vec3)> {
    let right = Vec3::new(-dir.y, dir.x, 0.0).normalize_or_zero();
    let up = dir.cross(right).normalize_or_zero();
    let mut rings: Vec<Vec<Vec3>> = vec![vec![dir]];
    if spread > 0.0 {
        for ring in 1..=2 {
            let r = spread * ring as f32;
            let n = 8 * ring;
            rings.push(
                (0..n)
                    .map(|k| {
                        let a = k as f32 / n as f32 * std::f32::consts::TAU;
                        (dir + right * (a.cos() * r) + up * (a.sin() * r)).normalize()
                    })
                    .collect(),
            );
        }
    }
    let candidates: Vec<(usize, usize, glam::Mat4, Vec<u32>)> = vehicle
        .trailers
        .iter()
        .enumerate()
        .flat_map(|(ti, trailer)| {
            let o = (origin - trailer.position).as_vec3();
            trailer
                .ty
                .meshes
                .iter()
                .enumerate()
                .filter_map(move |(i, vm)| {
                    if trailer.ty.model.meshes[vm.def_index].mouse_event.is_none()
                        || !trailer.mesh_props[i].visible
                    {
                        return None;
                    }
                    let xf = trailer.mesh_local_transform(i);
                    if !ray_may_hit(&trailer.ty, i, &xf, o, dir, spread * 2.2) {
                        return None;
                    }
                    let tris =
                        ::geometry::cone_triangles(o, dir, spread * 2.0 + 1e-4, &vm.data, &xf);
                    (!tris.is_empty()).then_some((ti, i, xf, tris))
                })
        })
        .collect();
    for dirs in &rings {
        let mut best: Option<(f32, usize, usize, Vec3)> = None;
        for (ti, i, xf, tris) in &candidates {
            let (ti, i, xf) = (*ti, *i, *xf);
            let trailer = &vehicle.trailers[ti];
            let o = (origin - trailer.position).as_vec3();
            let vm = &trailer.ty.meshes[i];
            for d in dirs {
                if let Some(t) = ::geometry::ray_triangles(o, *d, &vm.data, &xf, tris) {
                    if best.map(|(bt, _, _, _)| t < bt).unwrap_or(true) {
                        best = Some((t, ti, i, *d));
                    }
                }
            }
        }
        if let Some((_, ti, i, d)) = best {
            return Some(((ti, i), d));
        }
    }
    None
}

pub(super) fn encloses_origin(
    ty: &::simulation::VehicleType,
    i: usize,
    xf: &glam::Mat4,
    o: Vec3,
    dir: Vec3,
) -> bool {
    let vm = &ty.meshes[i];
    if ty.model.meshes[vm.def_index].mouse_event.is_some() {
        return false;
    }
    if let Some(&(c, r)) = ty.mesh_bounds.get(i) {
        let scale = xf
            .x_axis
            .truncate()
            .length()
            .max(xf.y_axis.truncate().length())
            .max(xf.z_axis.truncate().length());
        if (xf.transform_point3(c) - o).length() > r * scale + 0.05 {
            return false;
        }
    }
    let d = &vm.data;
    let mut hits = 0u32;
    for tri in d.indices.chunks_exact(3) {
        let p = |k: usize| xf.transform_point3(d.positions[tri[k] as usize]);
        if let Some(t) = ::geometry::ray_triangle(o, dir, p(0), p(1), p(2)) {
            if t > 0.0 {
                hits += 1;
            }
        }
    }
    hits % 2 == 1
}

pub(super) fn in_front_of(other: f32, control: f32) -> bool {
    other < control - 0.03
}

#[cfg(test)]
mod occlusion_tests {
    use super::in_front_of;
    use ::geometry::{MeshData, ray_mesh};
    use glam::{Mat4, Vec3};

    fn wall(z: f32) -> MeshData {
        MeshData {
            positions: vec![
                Vec3::new(-1.0, z, -1.0),
                Vec3::new(1.0, z, -1.0),
                Vec3::new(0.0, z, 1.0),
            ],
            indices: vec![0, 1, 2],
            ..Default::default()
        }
    }

    #[test]
    fn a_dashboard_in_front_of_a_hatch_takes_the_click() {
        let (o, d) = (Vec3::ZERO, Vec3::Y);
        let hatch = ray_mesh(o, d, &wall(2.0), &Mat4::IDENTITY).unwrap();
        let dash = ray_mesh(o, d, &wall(0.6), &Mat4::IDENTITY).unwrap();
        assert!(in_front_of(dash, hatch));
        assert!(!in_front_of(hatch, dash));
    }

    #[test]
    fn a_button_flush_with_its_panel_stays_reachable() {
        assert!(!in_front_of(1.0, 1.005));
        assert!(!in_front_of(1.0, 1.0));
        assert!(!in_front_of(f32::INFINITY, 1.0));
    }
}
